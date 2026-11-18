use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    filenames: HashMap<String, String>,
    extensions: HashMap<String, String>,
    templates: HashMap<String, String>,
}

impl Config {
    fn load_config() -> Result<Self, String> {
        let mut config: Config = toml::from_str(include_str!("../defaults.toml"))
            .map_err(|e| format!("Invalid default config: {e}"))?;
        if let Some(path) = user_config_path() {
            match fs::read_to_string(&path) {
                Ok(text) => {
                    let custom_config: Config = toml::from_str(&text)
                        .map_err(|e| format!("Invalid config {}: {e}", path.display()))?;
                    config.apply_overrides(custom_config);
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Failed to read config {}: {e}", path.display())),
            }
        }

        Ok(config)
    }

    fn apply_overrides(&mut self, custom_config: Self) {
        self.filenames.extend(custom_config.filenames);
        self.extensions.extend(custom_config.extensions);
        self.templates.extend(custom_config.templates);
    }

    fn get_template(&self, path: &Path) -> Result<Option<&str>, String> {
        let filename = path.file_name().and_then(|name| name.to_str());
        let extension = path.extension().and_then(|ext| ext.to_str());

        let rule = filename
            .and_then(|filename| {
                self.filenames
                    .get(filename)
                    .map(|template| (filename, template))
            })
            .or_else(|| {
                extension.and_then(|ext| self.extensions.get(ext).map(|template| (ext, template)))
            });

        // for the case didn't find template in both filenames and extensions
        let Some((name, template)) = rule else {
            return Ok(None);
        };

        // for the case didn't find content in template entries, aka uncomplete template
        let Some(content) = self.templates.get(template) else {
            return Err(format!("Rule {name:?} misses template {template:?}"));
        };

        Ok(Some(content.as_str()))
    }
}

fn user_config_path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join(".config"))
        })
        .map(|dir| dir.join("tmpf/templates.toml"))
}

fn main() {
    let mut args = env::args_os();

    let Some(path) = args.nth(1) else {
        eprintln!("Usage: tmpf <file>");
        std::process::exit(1);
    };

    let config = match Config::load_config() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(4);
        }
    };

    let contents = match config.get_template(Path::new(&path)) {
        Ok(None) => b"",
        Ok(Some(ctt)) => ctt.as_bytes(),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(5);
        }
    };

    let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to create file {}: {e}", path.to_string_lossy());
            std::process::exit(2);
        }
    };

    if let Err(e) = file.write_all(contents) {
        eprintln!("Failed to write to file {}: {e}", path.to_string_lossy());
        std::process::exit(3);
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Path};

    #[test]
    fn user_can_override_just_template_bady() {
        let mut config: Config = toml::from_str(include_str!("../defaults.toml")).unwrap();

        let custom_config: Config = toml::from_str(
            r#"
            [templates]
            cpp = "// user's cpp template"
        "#,
        )
        .unwrap();

        config.apply_overrides(custom_config);

        assert_eq!(
            config.get_template(Path::new("src/example.cc")),
            Ok(Some("// user's cpp template"))
        );
    }

    #[test]
    fn exact_filename_takes_precedence_over_extension() {
        let config: Config = toml::from_str(
            r#"
            [filenames]
            "special.cpp" = "special"

            [extensions]
            cpp = "cpp"

            [templates]
            special = "// exact filename template"
            cpp = "// extension template"
        "#,
        )
        .unwrap();

        assert_eq!(
            config.get_template(Path::new("src/special.cpp")),
            Ok(Some("// exact filename template"))
        );
    }

    #[test]
    fn unknown_extension_or_missing_template() {
        let mut config: Config = toml::from_str(include_str!("../defaults.toml")).unwrap();

        assert_eq!(config.get_template(Path::new("src/example.xyz")), Ok(None));

        let custom_config: Config = toml::from_str(
            r#"
            [extensions]
            xyz = "missing"
        "#,
        )
        .unwrap();

        config.apply_overrides(custom_config);

        assert_eq!(
            config.get_template(Path::new("src/example.xyz")),
            Err(r#"Rule "xyz" misses template "missing""#.to_string())
        );
    }
}
