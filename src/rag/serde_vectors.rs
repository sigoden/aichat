use super::*;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{de, Deserializer, Serializer};

pub fn serialize<S>(
    vectors: &IndexMap<DocumentId, Vec<f32>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded_map: IndexMap<String, String> = vectors
        .iter()
        .map(|(id, vec)| {
            let (h, l) = id.split();
            let byte_slice = unsafe {
                std::slice::from_raw_parts(
                    vec.as_ptr() as *const u8,
                    vec.len() * std::mem::size_of::<f32>(),
                )
            };
            (format!("{h}-{l}"), STANDARD.encode(byte_slice))
        })
        .collect();

    encoded_map.serialize(serializer)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<IndexMap<DocumentId, Vec<f32>>, D::Error>
where
    D: Deserializer<'de>,
{
    let encoded_map: IndexMap<String, String> =
        IndexMap::<String, String>::deserialize(deserializer)?;

    let mut decoded_map = IndexMap::new();
    for (key, base64_str) in encoded_map {
        let decoded_key: DocumentId = key
            .split_once('-')
            .and_then(|(h, l)| {
                let h = h.parse::<usize>().ok()?;
                let l = l.parse::<usize>().ok()?;
                Some(DocumentId::new(h, l))
            })
            .ok_or_else(|| de::Error::custom(format!("Invalid key '{key}'")))?;

        let decoded_data = STANDARD.decode(&base64_str).map_err(de::Error::custom)?;

        if decoded_data.len() % std::mem::size_of::<f32>() != 0 {
            return Err(de::Error::custom(format!("Invalid vector at '{key}'")));
        }

        let num_f32s = decoded_data.len() / std::mem::size_of::<f32>();

        let mut vec_f32 = vec![0.0f32; num_f32s];
        unsafe {
            std::ptr::copy_nonoverlapping(
                decoded_data.as_ptr(),
                vec_f32.as_mut_ptr() as *mut u8,
                decoded_data.len(),
            );
        }

        decoded_map.insert(decoded_key, vec_f32);
    }

    Ok(decoded_map)
}
