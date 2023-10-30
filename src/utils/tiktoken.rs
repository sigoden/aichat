//! Use tiktoken for count tokens
//!
//! Fork from https://github.com/dust-tt/dust/tree/main/core/src/providers/tiktoken

#![allow(unused)]

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use fancy_regex::Regex;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use rustc_hash::FxHashMap as HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::task;

pub fn cl100k_base() -> Result<CoreBPE> {
    let cl100k_base = include_str!("../../assets/cl100k_base.tiktoken");

    let mut encoder = HashMap::default();
    for line in cl100k_base.lines() {
        let mut parts = line.split(' ');
        let raw = parts.next().unwrap();
        let token = &general_purpose::STANDARD.decode(raw)?;
        let rank: usize = parts.next().unwrap().parse().unwrap();
        encoder.insert(token.clone(), rank);
    }

    let mut special_tokens = HashMap::default();
    special_tokens.insert(String::from("<|endoftext|>"), 100257);
    special_tokens.insert(String::from("<|fim_prefix|>"), 100258);
    special_tokens.insert(String::from("<|fim_middle|>"), 100259);
    special_tokens.insert(String::from("<|fim_suffix|>"), 100260);
    special_tokens.insert(String::from("<|endofprompt|>"), 100276);

    CoreBPE::new(
        encoder,
        special_tokens,
        "(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\\r\\n\\p{L}\\p{N}]?\\p{L}+|\\p{N}{1,3}| ?[^\\s\\p{L}\\p{N}]+[\\r\\n]*|\\s*[\\r\\n]+|\\s+(?!\\S)|\\s+",
    )
}

pub fn cl100k_base_singleton() -> Arc<Mutex<CoreBPE>> {
    lazy_static! {
        static ref CL100K_BASE: Arc<Mutex<CoreBPE>> = Arc::new(Mutex::new(cl100k_base().unwrap()));
    }
    CL100K_BASE.clone()
}

pub async fn decode_async(bpe: Arc<Mutex<CoreBPE>>, tokens: Vec<usize>) -> Result<String> {
    task::spawn_blocking(move || bpe.lock().decode(tokens)).await?
}

pub async fn encode_async(bpe: Arc<Mutex<CoreBPE>>, text: &str) -> Result<Vec<usize>> {
    let text = text.to_string();
    let r = task::spawn_blocking(move || bpe.lock().encode_with_special_tokens(&text)).await?;
    Ok(r)
}

pub async fn tokenize_async(bpe: Arc<Mutex<CoreBPE>>, text: &str) -> Result<Vec<(usize, String)>> {
    let text = text.to_string();
    let r = task::spawn_blocking(move || bpe.lock().tokenize(&text)).await?;
    Ok(r)
}

fn _byte_pair_merge(piece: &[u8], ranks: &HashMap<Vec<u8>, usize>) -> Vec<std::ops::Range<usize>> {
    let mut parts: Vec<_> = (0..piece.len()).map(|i| i..i + 1).collect();

    // If you have n parts and m merges, this does O(mn) work
    // We could do something with a heap and do O(m log n) work

    // Note that we hash bytes, not token pairs. As long as we train BPE the way we
    // currently do, this is equivalent. An easy way to break this would be to decouple
    // merge priority from token index or to prevent specific token merges.
    loop {
        if parts.len() == 1 {
            break;
        }
        let mut min_rank: Option<(usize, usize)> = None;
        for i in 0..parts.len() - 1 {
            let rank = if let Some(r) = ranks.get(&piece[parts[i].start..parts[i + 1].end]) {
                *r
            } else {
                continue;
            };
            if min_rank.is_none() || rank < min_rank.unwrap().0 {
                min_rank = Some((rank, i));
            }
        }
        if let Some((_, i)) = min_rank {
            parts[i] = parts[i].start..parts[i + 1].end;
            parts.remove(i + 1);
        } else {
            break;
        }
    }
    parts
}

pub fn byte_pair_encode(piece: &[u8], ranks: &HashMap<Vec<u8>, usize>) -> Vec<usize> {
    if piece.len() == 1 {
        return vec![ranks[piece]];
    }
    _byte_pair_merge(piece, ranks)
        .iter()
        .map(|p| ranks[&piece[p.start..p.end]])
        .collect()
}

pub fn byte_pair_split<'a>(piece: &'a [u8], ranks: &HashMap<Vec<u8>, usize>) -> Vec<&'a [u8]> {
    if piece.len() == 1 {
        return vec![piece];
    }
    _byte_pair_merge(piece, ranks)
        .iter()
        .map(|p| &piece[p.start..p.end])
        .collect()
}

// Various performance notes:
//
// Regex
// =====
// Most of the time is spent in regex. The easiest way to speed this up is by using less fancy
// regex features. For instance, using a regex parse-able by `regex` crate is 3x faster than
// the usual regex we use.
//
// However, given that we're using a regex parse-able by `regex`, there isn't much difference
// between using the `regex` crate and using the `fancy_regex` crate.
//
// Caching
// =======
// The reference tokeniser has an lru cache over the equivalent of `byte_pair_encode`.
// Originally, we had one too! Without it, we were only vaguely faster than Python.
// I used an RWLock to protect the cache. This didn't seem to hurt single threaded performance
// noticeably, but it did affect multi-threaded performance. Weirdly, it seemed to affect
// multi-threaded performance even when I only had readers (maybed I messed something up?).
// Anyway, I realised that we could get rid of the cache, if we treat the set of tokens as a cache!
// These are exactly the set or merges that are likely to be hot. And now we don't have to think
// about interior mutability, memory use, or cloning.
//
// Hashing
// =======
// We use FxHashMap instead of the standard HashMap. This is maybe like a 5-10% win?
// The current implementation ends up doing a lot of hashing of bytes. In theory, this could be made
// to be hashing of two-tuples of ints, which looks like it may also be a couple percent faster.

pub struct CoreBPE {
    encoder: HashMap<Vec<u8>, usize>,
    special_tokens_encoder: HashMap<String, usize>,
    decoder: HashMap<usize, Vec<u8>>,
    special_tokens_decoder: HashMap<usize, Vec<u8>>,
    regex: Regex,
    special_regex: Regex,
    sorted_token_bytes: Vec<Vec<u8>>,
}

impl CoreBPE {
    fn _get_regex(&self) -> &Regex {
        &self.regex
    }

    fn _get_special_regex(&self) -> &Regex {
        &self.special_regex
    }

    fn _decode_native(&self, tokens: &[usize]) -> Vec<u8> {
        let mut ret = Vec::with_capacity(tokens.len() * 2);
        for token in tokens {
            let token_bytes = self
                .decoder
                .get(token)
                .unwrap_or_else(|| &self.special_tokens_decoder[token]);
            ret.extend(token_bytes);
        }
        ret
    }

    fn _encode_ordinary_native(&self, text: &str) -> Vec<usize> {
        // This is the core of the encoding logic; the other functions in here
        // just make things complicated :-)
        let regex = self._get_regex();
        let mut ret = vec![];
        for mat in regex.find_iter(text) {
            let piece = mat.unwrap().as_str().as_bytes();
            if let Some(token) = self.encoder.get(piece) {
                ret.push(*token);
                continue;
            }
            ret.extend(&byte_pair_encode(piece, &self.encoder));
        }
        ret
    }

    fn _tokenize(&self, text: &str) -> Vec<(usize, String)> {
        let regex = self._get_regex();
        let mut results = vec![];

        for mat in regex.find_iter(text) {
            let string = mat.unwrap().as_str();
            let piece = string.as_bytes();
            if let Some(token) = self.encoder.get(piece) {
                results.push((*token, string.to_string()));
                continue;
            }

            results.extend(Self::_tokenize_byte_pair_encode(piece, &self.encoder));
        }
        results
    }

    // Copy of _encode_native but returns both the tokens and the associated string in a tuple
    // As needed in tokenize function
    fn _tokenize_with_spe_regex(
        &self,
        text: &str,
        allowed_special: &HashSet<&str>,
    ) -> Vec<(usize, String)> {
        let special_regex = self._get_special_regex();
        let regex = self._get_regex();
        let mut ret = vec![];

        let mut start = 0;
        loop {
            let mut next_special;
            let mut start_find = start;
            loop {
                // Find the next allowed special token, if any
                next_special = special_regex.find_from_pos(text, start_find).unwrap();
                match next_special {
                    Some(m) => {
                        if allowed_special.contains(&text[m.start()..m.end()]) {
                            break;
                        }
                        start_find = m.start() + 1;
                    }
                    None => break,
                }
            }
            let end = next_special.map_or(text.len(), |m| m.start());

            // Okay, here we go, compare this logic to _encode_ordinary_native
            for mat in regex.find_iter(&text[start..end]) {
                let string = mat.unwrap().as_str();
                let piece = string.as_bytes();
                if let Some(token) = self.encoder.get(piece) {
                    ret.push((*token, string.to_string()));
                    continue;
                }
                ret.extend(Self::_tokenize_byte_pair_encode(piece, &self.encoder));
            }

            match next_special {
                // And here we push the special token
                Some(m) => {
                    let piece = m.as_str();
                    let token = self.special_tokens_encoder[piece];
                    ret.push((token, piece.to_string()));
                    start = m.end();
                }
                None => break,
            }
        }
        ret
    }

    /**
     * Implemented to match the logic in _encode_ordinary_native
     * Used in tokenize function
     */
    pub fn _tokenize_byte_pair_encode(
        piece: &[u8],
        ranks: &HashMap<Vec<u8>, usize>,
    ) -> Vec<(usize, String)> {
        if piece.len() == 1 {
            let string = String::from_utf8_lossy(piece);
            return vec![(ranks[piece], string.to_string())];
        }

        _byte_pair_merge(piece, ranks)
            .iter()
            .map(|p| {
                (
                    ranks[&piece[p.start..p.end]],
                    String::from_utf8_lossy(&piece[p.start..p.end]).to_string(),
                )
            })
            .collect()
    }

    fn _encode_native(&self, text: &str, allowed_special: &HashSet<&str>) -> (Vec<usize>, usize) {
        let special_regex = self._get_special_regex();
        let regex = self._get_regex();
        let mut ret = vec![];

        let mut start = 0;
        let mut last_piece_token_len = 0;
        loop {
            let mut next_special;
            let mut start_find = start;
            loop {
                // Find the next allowed special token, if any
                next_special = special_regex.find_from_pos(text, start_find).unwrap();
                match next_special {
                    Some(m) => {
                        if allowed_special.contains(&text[m.start()..m.end()]) {
                            break;
                        }
                        start_find = m.start() + 1;
                    }
                    None => break,
                }
            }
            let end = next_special.map_or(text.len(), |m| m.start());

            // Okay, here we go, compare this logic to _encode_ordinary_native
            for mat in regex.find_iter(&text[start..end]) {
                let piece = mat.unwrap().as_str().as_bytes();
                if let Some(token) = self.encoder.get(piece) {
                    last_piece_token_len = 1;
                    ret.push(*token);
                    continue;
                }
                let tokens = byte_pair_encode(piece, &self.encoder);
                last_piece_token_len = tokens.len();
                ret.extend(&tokens);
            }

            match next_special {
                // And here we push the special token
                Some(m) => {
                    let piece = m.as_str();
                    let token = self.special_tokens_encoder[piece];
                    ret.push(token);
                    start = m.end();
                    last_piece_token_len = 0;
                }
                None => break,
            }
        }

        // last_piece_token_len is how many tokens came from the last regex split. This is used
        // for determining unstable tokens, since you can't merge across (stable) regex splits
        (ret, last_piece_token_len)
    }

    fn _increase_last_piece_token_len(
        &self,
        tokens: Vec<usize>,
        mut last_piece_token_len: usize,
    ) -> (Vec<usize>, usize) {
        // Unfortunately, the locations where our regex splits can be unstable.
        // For the purposes of determining unstable tokens, unstable regex splitting
        // is only a problem if a split that was present disappears, since this can
        // lead to merging of tokens otherwise thought to be stable.
        // cl100k_base makes our life hard by including the \s*[\r\n]+
        // pattern. This can e.g. cause "\n" + " " to become "\n \n".
        // Here is a quick and dirty fix:
        {
            let token_is_all_space = |token| {
                self.decoder
                    .get(token)
                    .map(|token_bytes| {
                        token_bytes
                            .iter()
                            .rev()
                            .all(|&b| [b' ', b'\n', b'\t'].contains(&b))
                    })
                    .unwrap_or(false)
            };
            if last_piece_token_len > 0
                && token_is_all_space(&tokens[tokens.len() - last_piece_token_len])
            {
                while (last_piece_token_len < tokens.len())
                    && token_is_all_space(&tokens[tokens.len() - last_piece_token_len - 1])
                {
                    last_piece_token_len += 1;
                }
            }
        }
        debug_assert!(last_piece_token_len <= tokens.len());

        (tokens, last_piece_token_len)
    }

    fn _encode_unstable_native(
        &self,
        text: &str,
        allowed_special: &HashSet<&str>,
    ) -> (Vec<usize>, HashSet<Vec<usize>>) {
        let (tokens, last_piece_token_len) = self._encode_native(text, allowed_special);
        if last_piece_token_len == 0 {
            // If last_piece_token_len is zero, the last token was a special token and we have
            // no unstable bytes
            return (tokens, HashSet::new());
        }
        let (mut tokens, last_piece_token_len) =
            self._increase_last_piece_token_len(tokens, last_piece_token_len);

        let unstable_bytes = self._decode_native(&tokens[tokens.len() - last_piece_token_len..]);
        tokens.truncate(tokens.len() - last_piece_token_len);

        // TODO: we should try harder to find additional stable tokens
        // This would reduce the amount of retokenising when determining completions
        // Refer to the logic in an older version of this file

        let mut completions = HashSet::new();
        if unstable_bytes.is_empty() {
            return (tokens, completions);
        }

        // This is the easy bit. Just find all single tokens that start with unstable_bytes
        // (including tokens that exactly match unstable_bytes)
        // Separating this from the loop below helps with performance in a common case.
        let mut point = self
            .sorted_token_bytes
            .partition_point(|x| x.as_slice() < unstable_bytes.as_slice());
        while point < self.sorted_token_bytes.len()
            && self.sorted_token_bytes[point].starts_with(&unstable_bytes)
        {
            completions.insert(vec![
                self.encoder[self.sorted_token_bytes[point].as_slice()],
            ]);
            point += 1;
        }

        // Now apply even more brute force. At every (other) possible position for the straddling
        // token, concatenate additional bytes from that token (if any) to unstable_bytes,
        // and retokenise the whole thing and see what we get.
        for i in 1..unstable_bytes.len() {
            let prefix = &unstable_bytes[..i];
            let suffix = &unstable_bytes[i..];
            let mut point = self
                .sorted_token_bytes
                .partition_point(|x| x.as_slice() < suffix);
            // TODO: Perf optimisation if suffix starts with " "?
            while point < self.sorted_token_bytes.len()
                && self.sorted_token_bytes[point].starts_with(suffix)
            {
                let possibility = [prefix, self.sorted_token_bytes[point].as_slice()].concat();
                let encoded = match std::str::from_utf8(&possibility) {
                    // Morally, this is byte_pair_encode(&possibility, &self.encoder)
                    // But we might have introduced a regex split which would prevent merges.
                    // (particularly possible in the presence of unstable regex splits)
                    // So convert to UTF-8 and do regex splitting.
                    // E.g. with cl100k_base "  !" gets split to " " + " !",
                    // but byte_pair_encode("  !") != byte_pair_encode(" ")
                    Ok(s) => self._encode_ordinary_native(s),

                    // Technically, whether or not this arm is correct depends on whether there
                    // would be a regex split before the UTF-8 truncation point.
                    // Probably niche enough that no one will ever notice (after all, people didn't
                    // notice all the big holes in the previous unstable token implementation)
                    Err(_) => byte_pair_encode(&possibility, &self.encoder),
                    // Something like the following is intriguing but incorrect:
                    // Err(e) => self._encode_ordinary_native(unsafe {
                    //     std::str::from_utf8_unchecked(&possibility[..e.valid_up_to()])
                    // }),
                };
                let mut seq = Vec::new();
                let mut seq_len = 0;
                for token in encoded {
                    seq.push(token);
                    seq_len += self.decoder[&token].len();
                    if seq_len >= unstable_bytes.len() {
                        break;
                    }
                }
                completions.insert(seq);
                point += 1;
            }
        }

        // This is also not straightforward. While we generally assume that regex splits are stable,
        // unfortunately, they are not. That is, if adding bytes were to make a split appear in
        // unstable_bytes, this could make tokens possible which our logic would otherwise think
        // would be merged.
        // For example, with gpt2, the use of \s+(?!\S) means that "\n\n" could
        // develop a split, e.g. "\n\n0" splits into "\n"+"\n"+"0", making "\n" a possible token.
        // Here is a quick and dirty fix:
        // This isn't right if we ever remove \s+(?!\S)
        if unstable_bytes.len() > 1 {
            let last_decoded = bstr::decode_last_utf8(unstable_bytes.as_slice());
            if unstable_bytes.len() - last_decoded.1 > 0
                && last_decoded.0.map_or(false, |c| c.is_whitespace())
            {
                let mut reencoded = byte_pair_encode(
                    &unstable_bytes[..unstable_bytes.len() - last_decoded.1],
                    &self.encoder,
                );
                reencoded.extend(byte_pair_encode(
                    &unstable_bytes[unstable_bytes.len() - last_decoded.1..],
                    &self.encoder,
                ));
                completions.insert(reencoded);
            }
        }

        (tokens, completions)
    }
}

impl CoreBPE {
    fn new(
        encoder: HashMap<Vec<u8>, usize>,
        special_tokens_encoder: HashMap<String, usize>,
        pattern: &str,
    ) -> Result<Self> {
        let regex = Regex::new(pattern)?;

        let special_regex = {
            let _parts = special_tokens_encoder
                .keys()
                .map(|s| fancy_regex::escape(s))
                .collect::<Vec<_>>();
            Regex::new(&_parts.join("|"))?
        };

        let decoder: HashMap<usize, Vec<u8>> =
            encoder.iter().map(|(k, v)| (*v, k.clone())).collect();

        assert!(encoder.len() == decoder.len());

        let special_tokens_decoder: HashMap<usize, Vec<u8>> = special_tokens_encoder
            .iter()
            .map(|(k, v)| (*v, k.as_bytes().to_vec()))
            .collect();

        // Clone because I don't know how to tell Rust I'm not going to change the map
        let mut sorted_token_bytes: Vec<Vec<u8>> = encoder.keys().cloned().collect();
        sorted_token_bytes.sort();

        Ok(CoreBPE {
            encoder,
            special_tokens_encoder,
            decoder,
            special_tokens_decoder,
            regex,
            special_regex,
            sorted_token_bytes,
        })
    }

    // ====================
    // Encoding
    // ====================

    pub fn encode_ordinary(&self, text: &str) -> Vec<usize> {
        self._encode_ordinary_native(text)
    }

    pub fn encode(&self, text: &str, allowed_special: HashSet<&str>) -> Vec<usize> {
        self._encode_native(text, &allowed_special).0
    }

    pub fn tokenize(&self, text: &str) -> Vec<(usize, String)> {
        let allowed_special = self
            .special_tokens_encoder
            .keys()
            .map(|s| s.as_str())
            .collect();
        self._tokenize_with_spe_regex(text, &allowed_special)
    }

    pub fn encode_with_special_tokens(&self, text: &str) -> Vec<usize> {
        let allowed_special = self
            .special_tokens_encoder
            .keys()
            .map(|s| s.as_str())
            .collect();
        self._encode_native(text, &allowed_special).0
    }

    fn _encode_bytes(&self, bytes: &[u8]) -> Vec<usize> {
        match std::str::from_utf8(bytes) {
            Ok(text) => self._encode_ordinary_native(text),
            Err(e) => {
                let text = unsafe { std::str::from_utf8_unchecked(&bytes[..e.valid_up_to()]) };
                let (tokens, last_piece_token_len) = self._encode_native(text, &HashSet::new());
                let (mut tokens, last_piece_token_len) =
                    self._increase_last_piece_token_len(tokens, last_piece_token_len);
                if !tokens.is_empty() && last_piece_token_len > 0 {
                    // Lop off the tokens from the last piece and run BPE on the remaining bytes
                    // Somewhat niche, but this may not be correct if we'd have had a regex
                    // split between the valid UTF-8 and the invalid bytes, which is why this
                    // method is private
                    let mut unstable_bytes =
                        self._decode_native(&tokens[tokens.len() - last_piece_token_len..]);
                    unstable_bytes.extend_from_slice(&bytes[e.valid_up_to()..]);

                    tokens.truncate(tokens.len() - last_piece_token_len);
                    tokens.extend(byte_pair_encode(&unstable_bytes, &self.encoder));
                }
                tokens
            }
        }
    }

    #[allow(dead_code)]
    fn encode_with_unstable(
        &self,
        text: &str,
        allowed_special: HashSet<&str>,
    ) -> (Vec<usize>, HashSet<Vec<usize>>) {
        self._encode_unstable_native(text, &allowed_special)
    }

    #[allow(dead_code)]
    fn encode_single_token(&self, piece: &[u8]) -> Result<usize> {
        if let Some(token) = self.encoder.get(piece).copied() {
            return Ok(token);
        }
        if let Ok(piece_str) = std::str::from_utf8(piece) {
            if let Some(token) = self.special_tokens_encoder.get(piece_str).copied() {
                return Ok(token);
            }
        }
        Err(anyhow!("Token not found in the vocabulary: {:?}", piece))
    }

    #[allow(dead_code)]
    fn encode_single_piece(&self, piece: &[u8]) -> Vec<usize> {
        if let Some(token) = self.encoder.get(piece) {
            return vec![*token];
        }
        byte_pair_encode(piece, &self.encoder)
    }

    // ====================
    // Decoding
    // ====================

    pub fn decode_bytes(&self, tokens: Vec<usize>) -> Vec<u8> {
        self._decode_native(&tokens)
    }

    pub fn decode(&self, tokens: Vec<usize>) -> Result<String> {
        match String::from_utf8(self._decode_native(&tokens)) {
            Ok(text) => Ok(text),
            Err(e) => Err(anyhow!("Unable to decode into a valid UTF-8 string: {}", e)),
        }
    }

    pub fn decode_single_token_bytes(&self, token: usize) -> Result<Vec<u8>> {
        if let Some(bytes) = self.decoder.get(&token) {
            return Ok(bytes.clone());
        }
        if let Some(bytes) = self.special_tokens_decoder.get(&token) {
            return Ok(bytes.clone());
        }
        Err(anyhow!("Token not found in the vocabulary: {}", token))
    }

    // ====================
    // Miscellaneous
    // ====================

    #[allow(dead_code)]
    fn token_byte_values(&self) -> Vec<Vec<u8>> {
        self.sorted_token_bytes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rustc_hash::FxHashMap as HashMap;

    #[test]
    fn very_simple_test() {
        let mut ranks = HashMap::default();
        ranks.insert(b"ab".to_vec(), 1);
        ranks.insert(b"cd".to_vec(), 2);

        let res = byte_pair_split(b"abcd", &ranks);
        assert_eq!(res, vec![b"ab", b"cd"]);
    }

    #[test]
    fn cl100k_base_test() {
        let bpe = cl100k_base().unwrap();
        let tokens = bpe.encode_with_special_tokens("This is a test         with a lot of spaces");
        let decoded = bpe.decode(tokens.clone()).unwrap();
        assert_eq!(decoded, "This is a test         with a lot of spaces");
        assert_eq!(
            tokens,
            vec![2028, 374, 264, 1296, 260, 449, 264, 2763, 315, 12908]
        );
    }
}
