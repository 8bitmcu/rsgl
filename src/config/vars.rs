//! `{name}` variables: `[vars]`, a game's own `vars`, and how they're filled in.

use std::collections::BTreeMap;

/// `{env:NAME}` is environment variable NAME, see `Vars::expand_env`.
const ENV_PREFIX: &str = "env:";

/// Letters, digits and `_`: what `{env:NAME}` and a top bar toggle accept.
pub fn is_env_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// What `{name}` accepts: letters, digits, `_` and `-`.
pub fn is_var_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// `text` as a variable's value that's used as it is: its braces are literal.
pub(super) fn literal(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

/// A name with `{year}` blank or filled in, cleaned up: "Quake ({year})" becomes
/// "Quake" rather than "Quake ()" while the year isn't known.
pub fn tidy(name: &str) -> String {
    name.replace("()", "").replace("[]", "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Values for `{name}` placeholders: the global `[vars]` with a game's own `vars` on top.
/// Numbers and booleans are used as their text, and a value may itself contain `{other}`,
/// which is looked up after the two tables are merged, so a game's override reaches it.
#[derive(Clone, Default)]
pub struct Vars(BTreeMap<String, String>);

impl Vars {
    /// Variables with these literal values; braces in them are not variables.
    pub fn literal<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vars {
        Vars::default().with_defaults(pairs)
    }

    /// These variables with `table` added on top.
    pub(super) fn with(&self, table: &BTreeMap<String, toml::Value>) -> Result<Vars, String> {
        let mut map = self.0.clone();
        for (name, value) in table {
            let text = match value {
                toml::Value::String(s) => s.clone(),
                toml::Value::Integer(n) => n.to_string(),
                toml::Value::Float(f) => f.to_string(),
                toml::Value::Boolean(b) => b.to_string(),
                _ => return Err(format!("{name} must be a string, number or boolean")),
            };
            map.insert(name.clone(), text);
        }
        Ok(Vars(map))
    }

    /// These variables with each `(name, value)` added, unless it's already set.
    /// The values are literal text: braces in them are not variables.
    pub fn with_defaults<'a>(&self, pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vars {
        let mut map = self.0.clone();
        for (name, value) in pairs {
            map.entry(name.to_string()).or_insert_with(|| literal(value));
        }
        Vars(map)
    }

    /// These variables with `name` set to `template`, unless it's already set. Unlike
    /// `with_defaults`, `template` is expanded like any other value.
    pub(super) fn with_template_default(&self, name: &str, template: &str) -> Vars {
        let mut map = self.0.clone();
        map.entry(name.to_string()).or_insert_with(|| template.to_string());
        Vars(map)
    }

    /// These variables with each `(name, value)` set, replacing what was there.
    /// The values are literal text, like in `with_defaults`.
    pub fn with_values<'a>(&self, pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vars {
        let mut map = self.0.clone();
        map.extend(pairs.into_iter().map(|(name, value)| (name.to_string(), literal(value))));
        Vars(map)
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Replaces every `{name}` in `s`. `{{` and `}}` are literal braces.
    pub fn expand(&self, s: &str) -> Result<String, String> {
        self.expand_in(s, &mut Vec::new(), None)
    }

    /// `expand`, where `{env:NAME}` is also allowed and is `env("NAME")`, literally.
    /// Only the bars have these: everything else runs through the shell, which has `$NAME`.
    pub fn expand_env(&self, s: &str, env: &dyn Fn(&str) -> String) -> Result<String, String> {
        self.expand_in(s, &mut Vec::new(), Some(env))
    }

    /// `chain` is the variables being expanded right now, outermost first, to catch
    /// loops and to say where an error is.
    fn expand_in(&self, s: &str, chain: &mut Vec<String>, env: Option<&dyn Fn(&str) -> String>) -> Result<String, String> {
        let at = chain.last().map(|v| format!(" (in variable {v})")).unwrap_or_default();
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(pos) = rest.find(['{', '}']) {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + 1..];
            if rest.as_bytes()[pos] == b'}' {
                let Some(after) = after.strip_prefix('}') else {
                    return Err(format!("stray }} in {s:?}{at} (write }}}} for a literal brace)"));
                };
                out.push('}');
                rest = after;
            } else if let Some(after) = after.strip_prefix('{') {
                out.push('{');
                rest = after;
            } else {
                let Some(end) = after.find('}') else {
                    return Err(format!("unclosed {{ in {s:?}{at} (write {{{{ for a literal brace)"));
                };
                let name = &after[..end];
                if let Some(var) = name.strip_prefix(ENV_PREFIX) {
                    if !is_env_name(var) {
                        return Err(format!("{{{name}}} isn't an environment variable name{at}"));
                    }
                    let Some(env) = env else {
                        return Err(format!("{{{name}}} only works in [status] and [topbar] labels{at}; use ${var} in commands"));
                    };
                    out.push_str(&env(var));
                } else if !is_var_name(name) {
                    return Err(format!(
                        "{{{name}}} isn't a variable name{at} (write {{{{ for a literal brace)"
                    ));
                } else {
                    out.push_str(&self.value(name, chain, &at, env)?);
                }
                rest = &after[end + 1..];
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    fn value(&self, name: &str, chain: &mut Vec<String>, at: &str, env: Option<&dyn Fn(&str) -> String>) -> Result<String, String> {
        chain.push(name.to_string());
        if chain[..chain.len() - 1].iter().any(|n| n == name) {
            return Err(format!("variables refer to each other in a circle: {}", chain.join(" -> ")));
        }
        let Some(raw) = self.0.get(name) else {
            return Err(format!("{{{name}}} isn't defined{at}"));
        };
        let text = self.expand_in(raw, chain, env)?;
        chain.pop();
        Ok(text)
    }
}
