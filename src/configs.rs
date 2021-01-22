use std::collections::HashMap;

use toml::Value;

use crate::lsp::LSConfig;

pub fn language_configs() -> HashMap<String, LSConfig> {
    let configs_src: String = include_str!("language_config.toml").to_string();
    let configs_value = configs_src.parse::<toml::Value>().unwrap();
    match configs_value {
        Value::Table(t) => t
            .into_iter()
            .map(|(name, v)| {
                let config: LSConfig = toml::from_str(&v.to_string()).unwrap();
                (name, config)
            })
            .collect(),
        _ => unreachable!("`languge_config.toml` is not valid"),
    }
}
