#![forbid(unsafe_code)]

use crate::errors::KonaError;
use crate::types::K;
use std::collections::HashMap;
use std::sync::RwLock;

pub fn create_dict(keys: &K, values: &K) -> Result<K, KonaError> {
    let keys_len = keys.len()?;
    let values_len = values.len()?;
    if keys_len != values_len {
        return Err(KonaError::Length);
    }
    Ok(K::dictionary(keys.clone(), values.clone()))
}

/// KTree holds global variables in a thread-safe way.
pub struct KTree {
    variables: RwLock<HashMap<String, K>>,
}

impl KTree {
    pub fn new() -> Self {
        let mut vars = HashMap::new();
        vars.insert("_n".to_string(), K::nil());
        vars.insert("_d".to_string(), K::symbol(".k"));
        vars.insert("_a".to_string(), K::symbol_list(vec![]));
        vars.insert("_p".to_string(), K::int(7));
        Self {
            variables: RwLock::new(vars),
        }
    }

    fn resolve(&self, path: &str) -> String {
        if path.starts_with('.') || path.starts_with('_') {
            path.to_string()
        } else {
            let d = self
                .lookup_raw("_d")
                .and_then(|k| k.as_symbol().map(|s| s.to_string()))
                .unwrap_or_else(|| ".k".to_string());
            if d == "." || d.is_empty() {
                path.to_string()
            } else {
                format!("{}.{}", d, path)
            }
        }
    }

    fn lookup_raw(&self, path: &str) -> Option<K> {
        let guard = self.variables.read().ok()?;
        guard.get(path).cloned()
    }

    pub fn lookup(&self, path: &str) -> Option<K> {
        let resolved = self.resolve(path);
        self.lookup_raw(&resolved)
    }

    pub fn assign(&self, path: String, val: K) {
        let resolved = self.resolve(&path);
        if resolved == "_p" {
            if let Some(p_val) = val.as_int() {
                if p_val >= 0 {
                    crate::io::PRINT_PRECISION
                        .store(p_val as usize, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        if let Ok(mut guard) = self.variables.write() {
            guard.insert(resolved, val);
        }
    }

    pub fn keys(&self) -> Vec<String> {
        if let Ok(guard) = self.variables.read() {
            guard.keys().cloned().collect()
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_creation() {
        let keys = K::int_list(vec![1, 2]);
        let values = K::symbol_list(vec!["a".into(), "b".into()]);
        let dict = create_dict(&keys, &values).unwrap();

        match &*dict {
            crate::types::KData::Dictionary { keys: k, values: v } => {
                assert_eq!(k, &keys);
                assert_eq!(v, &values);
            }
            _ => panic!("Expected a dictionary"),
        }

        let bad_values = K::symbol_list(vec!["a".into()]);
        assert!(create_dict(&keys, &bad_values).is_err());
    }

    #[test]
    fn test_ktree_thread_safety() {
        let ktree = KTree::new();
        ktree.assign(".k.cnt".to_string(), K::int(42));
        assert_eq!(ktree.lookup(".k.cnt").unwrap(), K::int(42));

        // Test concurrent access
        let ktree_arc = std::sync::Arc::new(ktree);
        let ktree_clone = ktree_arc.clone();

        let handle = std::thread::spawn(move || {
            ktree_clone.assign(".k.hist".to_string(), K::int(100));
        });

        handle.join().unwrap();
        assert_eq!(ktree_arc.lookup(".k.hist").unwrap(), K::int(100));
    }
}
