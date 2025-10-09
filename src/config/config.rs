use crate::err::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap as Map;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;
use structopt::lazy_static::lazy_static;

#[derive(Serialize, Deserialize, Debug)]
struct Identity {
    machine_name: String,

    private_key_loc: String,
    public_key_loc: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Connection {
    conn_token: String,
    port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
struct AppConfig {
    working_dir: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    identity: Identity,
    connection: Connection,
    app_config: AppConfig,
}

impl Config {
    pub fn new() -> Self {
        Config {
            identity: Identity {
                machine_name: String::from(""),
                private_key_loc: String::from(""),
                public_key_loc: String::from(""),
            },
            connection: Connection {
                conn_token: String::from(""),
                port: 0,
            },
            app_config: AppConfig {
                working_dir: String::from(""),
            },
        }
    }

    pub fn from_config(config_path: Option<&str>) -> Result<Self> {
        match config_path {
            Some(p) => {
                // Expand leading '~/'' HOME to support shell-like paths in config defaults
                let path = if p.starts_with("~/") {
                    match std::env::var("HOME") {
                        Ok(home) => format!("{}/{}", home, &p[2..]),
                        Err(_) => p.to_string(),
                    }
                } else {
                    p.to_string()
                };
                let content = fs::read_to_string(&path)?;
                match toml::from_str(&content) {
                    Ok(config) => Ok(config),
                    Err(e) => Err(e.into()),
                }
            }
            None => Err("No config file provided".into()),
        }
    }

    pub fn dump(&self, config_path: &str) -> Result<()> {
        let path = Path::new(config_path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let p = fs::File::create(path)?;
        let mut f_writer = std::io::BufWriter::new(p);
        f_writer.write_all(toml::to_string(&self)?.as_bytes())?;
        Ok(())
    }
}

enum ConfigInputValue {
    String(String),
    Int(i32),
    Uint(u32),
    Bool(bool),
}

impl ConfigInputValue {
    pub fn to_string(&self) -> String {
        match self {
            ConfigInputValue::String(s) => s.to_string(),
            ConfigInputValue::Int(i) => i.to_string(),
            ConfigInputValue::Uint(u) => u.to_string(),
            ConfigInputValue::Bool(b) => b.to_string(),
        }
    }
}

struct ConfigInput {
    name: String,
    pattern: String,
    description: String,
    default: Option<ConfigInputValue>,
}

impl ConfigInput {
    pub fn to_prompt(&self) -> String {
        let mut s = String::from(&self.description);
        if let Some(default) = &self.default {
            s.push_str(&format!(" (default: {})", default.to_string()));
        }
        s.push_str(": ");
        s
    }

    pub fn to_error_msg(&self, input: &str) -> String {
        format!(
            "expect input to follow pattern /{}/, the input '{}' does not meet requirements. please input again: ",
            self.pattern, input
        )
    }

    pub fn test_pattern(&self, input: &str) -> bool {
        if self.pattern.is_empty() {
            return true;
        }
        if self.default.is_some() && input.is_empty() {
            return true;
        }
        match Regex::new(&self.pattern) {
            Ok(re) => re.is_match(input),
            Err(err) => {
                eprintln!("Invalid regex in config: {}", err);
                false
            }
        }
    }
}

lazy_static! {
    static ref CONFIG_INPUT_LIST: Vec<ConfigInput> = vec!(
        ConfigInput {
            name: String::from("machine_name"),
            pattern: String::from(r"^[-a-zA-Z0-9_ @.*!$&()+,;=:]+$"),
            description: String::from("Please input your machine name"),
            default: None,
        },
        ConfigInput {
            name: String::from("private_key_location"),
            pattern: String::from(r"^[-0-9a-zA-Z_/.\\]+$"),
            description: String::from("Please input your private key location"),
            default: None,
        },
        ConfigInput {
            name: String::from("public_key_location"),
            pattern: String::from(r"^[-0-9a-zA-Z_/.\\]+$"),
            description: String::from("Please input your public key location"),
            default: None,
        },
        ConfigInput {
            name: String::from("conn_token"),
            pattern: String::from(r"^[0-9a-zA-Z]{1,64}$"),
            description: String::from(
                "Set a connection token for the server to use. All servers sharing the same token join in the same disc group. This should be a random string of 1-64 alphanumeric characters."
            ),
            default: None,
        },
        ConfigInput {
            name: String::from("port_number"),
            pattern: String::from(r"^[0-9]{1,5}$"),
            description: String::from("Set a port number for the server to listen on"),
            default: Some(ConfigInputValue::Uint(14514)),
        },
        ConfigInput {
            name: String::from("working_dir"),
            pattern: String::from(r"^[-0-9a-zA-Z_/.\\]+$"),
            description: String::from("Set a working directory of the shared disc"),
            default: None,
        }
    );
}

fn read_input(required_input: &ConfigInput) -> Result<(String, String)> {
    let mut input = String::new();
    print!("{}", required_input.to_prompt());
    std::io::stdout().flush()?;
    std::io::stdin().read_line(&mut input)?;
    input = input.trim().to_string();

    loop {
        if required_input.test_pattern(&input) {
            if input.is_empty() {
                if let Some(default) = &required_input.default {
                    return Ok((required_input.name.clone(), default.to_string()));
                }
            }
            return Ok((required_input.name.clone(), input));
        }
        print!("{}", required_input.to_error_msg(&input));
        std::io::stdout().flush()?;
        std::io::stdin().read_line(&mut input)?;
        input = input.trim().to_string();
    }
}

pub fn interactive_config_setup(default_config_path: &str) -> Result<Config> {
    let mut config = Config::new();

    let mut input_map = Map::<String, String>::new();

    // looping through the list of inputs, asking for each one and storing the result in a map
    for required_input in CONFIG_INPUT_LIST.iter() {
        let (name, input) = read_input(required_input)?;
        input_map.insert(name, input);
    }

    // last question: where do you want to store this config?
    let config_file_input = ConfigInput {
        name: String::from("config_path"),
        pattern: String::from(r"^[0-9a-zA-Z_/.-\\]+$"),
        description: String::from("Where do you want to store this config"),
        default: Some(ConfigInputValue::String(String::from(default_config_path))),
    };
    let (name, input) = read_input(&config_file_input)?;
    input_map.insert(name, input);

    config.identity.machine_name = input_map.remove("machine_name").unwrap();
    config.identity.private_key_loc = input_map.remove("private_key_location").unwrap();
    config.identity.public_key_loc = input_map.remove("public_key_location").unwrap();
    config.app_config.working_dir = input_map.remove("working_dir").unwrap();
    config.connection.conn_token = input_map.remove("conn_token").unwrap();
    config.connection.port = input_map
        .remove("port_number")
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let save_path_input = input_map.remove("config_path").unwrap();
    let save_path = if save_path_input.starts_with("~/") {
        match &std::env::var("HOME") {
            Ok(home) => format!("{}/{}", home, &save_path_input[2..]),
            Err(_) => save_path_input,
        }
    } else {
        save_path_input
    };

    // Persist the configuration as TOML. Parent directories are created in dump().
    config.dump(&save_path)?;

    Ok(config)
}

pub fn get_or_create_config(config_path: Option<&str>) -> Result<Config> {
    match Config::from_config(config_path) {
        Ok(config) => Ok(config),
        Err(e) => {
            if !std::io::stdin().is_terminal() {
                return Err("No configuration file found and stdin is not a TTY; run in a terminal to create a config or provide --config pointing to a valid file.".into());
            }
            interactive_config_setup(config_path.unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::Read;
    use std::path::PathBuf;

    fn unique_temp_path(file: &str) -> PathBuf {
        let mut p = env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("disc_server_test_{}", nanos));
        p.push(file);
        p
    }

    #[test]
    fn rust_version_regex() {
        let ci = ConfigInput {
            name: String::from("version"),
            pattern: String::from(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.+]+)?$"),
            description: String::from("Rust version"),
            default: None,
        };
        assert!(ci.test_pattern("1.80.0"));
        assert!(ci.test_pattern("1.80.0-nightly"));
        assert!(ci.test_pattern("1.81.0-beta.2"));
        assert!(!ci.test_pattern("1.8"));
        assert!(!ci.test_pattern("v1.80.0"));
    }

    #[test]
    fn prompt_formats_with_and_without_default() {
        let with_default = ConfigInput {
            name: "port_number".to_string(),
            pattern: String::new(),
            description: "Set a port".to_string(),
            default: Some(ConfigInputValue::Uint(8080)),
        };
        let without_default = ConfigInput {
            name: "name".to_string(),
            pattern: String::new(),
            description: "Machine name".to_string(),
            default: None,
        };
        let p1 = with_default.to_prompt();
        assert!(p1.contains("default: 8080"));
        assert!(p1.ends_with(": "));
        let p2 = without_default.to_prompt();
        assert!(!p2.contains("default:"));
        assert!(p2.ends_with(": "));
    }

    #[test]
    fn test_pattern_empty_and_invalid_regex() {
        let empty = ConfigInput {
            name: "any".to_string(),
            pattern: String::new(),
            description: String::new(),
            default: None,
        };
        assert!(empty.test_pattern("anything should pass"));

        // Invalid regex should be handled gracefully and return false
        let invalid = ConfigInput {
            name: "bad".to_string(),
            pattern: "[".to_string(),
            description: String::new(),
            default: None,
        };
        assert!(!invalid.test_pattern("doesn't matter"));
    }

    #[test]
    fn config_input_value_to_string_variants() {
        assert_eq!(ConfigInputValue::String("abc".into()).to_string(), "abc");
        assert_eq!(ConfigInputValue::Int(-12).to_string(), "-12");
        assert_eq!(ConfigInputValue::Uint(34).to_string(), "34");
        assert_eq!(ConfigInputValue::Bool(true).to_string(), "true");
    }

    #[test]
    fn dump_creates_parent_dirs_and_writes_toml() {
        let mut cfg = Config::new();
        cfg.identity.machine_name = "m1".into();
        cfg.identity.private_key_loc = "/tmp/priv".into();
        cfg.identity.public_key_loc = "/tmp/pub".into();
        cfg.connection.conn_token = "TOKEN".into();
        cfg.connection.port = 1234;

        let path = unique_temp_path("nested/config.toml");
        let parent = path.parent().unwrap();
        if parent.exists() {
            fs::remove_dir_all(parent).ok();
        }
        cfg.dump(path.to_str().unwrap())
            .expect("dump should succeed");
        assert!(path.exists());

        // Read back and validate some fields
        let mut s = String::new();
        fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        let loaded: Config = toml::from_str(&s).unwrap();
        assert_eq!(loaded.identity.machine_name, "m1");
        assert_eq!(loaded.connection.port, 1234);
    }

    #[test]
    fn from_config_expands_tilde_with_home() {
        // Prepare a temporary HOME
        let tmp_home = unique_temp_path("home_root");
        fs::create_dir_all(&tmp_home).unwrap();
        let prev_home = env::var_os("HOME");
        unsafe {
            env::set_var("HOME", &tmp_home);
        }
        let config_path = tmp_home.join("disc_config.toml");

        // Write a minimal valid config
        let mut cfg = Config::new();
        cfg.identity.machine_name = "local".into();
        cfg.identity.private_key_loc = "/k/priv".into();
        cfg.identity.public_key_loc = "/k/pub".into();
        cfg.connection.conn_token = "XYZ".into();
        cfg.connection.port = 8081;
        cfg.dump(config_path.to_str().unwrap()).unwrap();

        // Load using a path that starts with ~/ ...
        let loaded =
            Config::from_config(Some("~/disc_config.toml")).expect("should load via ~ expansion");
        assert_eq!(loaded.identity.machine_name, "local");
        assert_eq!(loaded.connection.port, 8081);

        // Restore HOME
        if let Some(prev) = prev_home {
            unsafe {
                env::set_var("HOME", prev);
            }
        } else {
            unsafe {
                env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn config_input_list_contains_expected_defaults() {
        // Ensure the default port appears in the prompt text
        let port_input = super::CONFIG_INPUT_LIST
            .iter()
            .find(|i| i.name == "port_number")
            .expect("port_number input present");
        let prompt = port_input.to_prompt();
        assert!(prompt.contains("14514"));
    }
}
