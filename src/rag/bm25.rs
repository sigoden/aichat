use rayon::prelude::*;
use std::collections::HashMap;
use std::f64;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone)]
pub struct BM25Options {
    k1: f64,
    b: f64,
    epsilon: f64,
}

impl Default for BM25Options {
    fn default() -> Self {
        Self {
            k1: 1.5,
            b: 0.75,
            epsilon: 0.25,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BM25<T> {
    options: BM25Options,
    corpus_size: usize,
    avgdl: f64,
    doc_freqs: Vec<HashMap<String, u32>>,
    doc_ids: Vec<T>,
    idf: HashMap<String, f64>,
    doc_len: Vec<usize>,
}

impl<T: Clone> BM25<T> {
    pub fn new(corpus: Vec<(T, String)>, options: BM25Options) -> Self {
        let mut doc_ids = vec![];
        let mut docs = vec![];
        for (id, value) in corpus {
            doc_ids.push(id);
            docs.push(value);
        }
        let tokenized_docs = docs.into_par_iter().map(|text| tokenize(&text)).collect();

        let mut bm25 = BM25 {
            options,
            corpus_size: 0,
            avgdl: 0.0,
            doc_freqs: Vec::new(),
            doc_ids,
            idf: HashMap::new(),
            doc_len: Vec::new(),
        };

        let map = bm25.initialize(tokenized_docs);
        bm25.calc_idf(map);

        bm25
    }

    pub fn search(&self, query: &str, top_k: usize, min_score: Option<f64>) -> Vec<T> {
        let scores = self.get_scores(query);
        let mut indexed_scores: Vec<(T, f64)> = scores
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| match min_score {
                Some(minimum_score) => {
                    if v < minimum_score {
                        None
                    } else {
                        Some((self.doc_ids[i].clone(), v))
                    }
                }
                None => Some((self.doc_ids[i].clone(), v)),
            })
            .collect();
        indexed_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        indexed_scores
            .into_iter()
            .take(top_k)
            .map(|(id, _)| id)
            .collect()
    }

    pub fn get_scores(&self, query: &str) -> Vec<f64> {
        let mut score = vec![0.0; self.corpus_size];

        for q in tokenize(query) {
            if let Some(idf) = self.idf.get(&q) {
                for (i, doc) in self.doc_freqs.iter().enumerate() {
                    let q_freq = doc.get(&q).unwrap_or(&0);
                    score[i] += *idf
                        * (*q_freq as f64 * (self.options.k1 + 1.0)
                            / (*q_freq as f64
                                + self.options.k1
                                    * (1.0 - self.options.b
                                        + self.options.b * self.doc_len[i] as f64 / self.avgdl)));
                }
            }
        }

        score
    }

    fn initialize(&mut self, corpus: Vec<Vec<String>>) -> HashMap<String, usize> {
        let mut map = HashMap::new();
        let mut num_doc = 0;

        for document in corpus {
            self.doc_len.push(document.len());
            num_doc += document.len();

            let mut frequencies = HashMap::new();
            for word in document {
                *frequencies.entry(word).or_insert(0) += 1;
            }
            self.doc_freqs.push(frequencies);

            for word in self.doc_freqs[self.doc_freqs.len() - 1].keys() {
                *map.entry(word.clone()).or_insert(0) += 1;
            }

            self.corpus_size += 1;
        }

        self.avgdl = num_doc as f64 / self.corpus_size as f64;
        map
    }

    fn calc_idf(&mut self, map: HashMap<String, usize>) {
        let mut idf_sum = 0.0;
        let mut negative_idfs = Vec::new();

        for (word, freq) in map {
            let idf = (self.corpus_size as f64 - freq as f64 + 0.5).ln() - (freq as f64 + 0.5).ln();
            self.idf.insert(word.clone(), idf);
            idf_sum += idf;
            if idf < 0.0 {
                negative_idfs.push(word);
            }
        }

        let average_idf = idf_sum / self.idf.len() as f64;

        for word in negative_idfs {
            self.idf.insert(word, self.options.epsilon * average_idf);
        }
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.unicode_words()
        .filter_map(|v| {
            if [
                "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "if", "in", "into",
                "is", "it", "no", "not", "of", "on", "or", "such", "that", "the", "their", "then",
                "there", "these", "they", "this", "to", "was", "will", "with",
            ]
            .contains(&v)
            {
                None
            } else {
                Some(v.to_string())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        assert_eq!(
            tokenize("a quick fox jumps over the lazy dog"),
            vec!["quick", "fox", "jumps", "over", "lazy", "dog"]
        );
    }

    #[test]
    fn test_bm25() {
        let corpus = vec![
            (0, "Hello there good man!".into()),
            (1, "It is quite windy in London".into()),
            (2, "How is the weather today?".into()),
        ];
        let bm25 = BM25::new(corpus, BM25Options::default());

        let scores = bm25.get_scores("windy London");
        assert_eq!(scores, [0.0, 0.9372947225064051, 0.0]);

        let top_n = bm25.search("windy London", 3, None);
        assert_eq!(top_n, vec![1, 0, 2])
    }
}
