#[cfg_attr(test, derive(serde::Serialize, Debug))]
pub enum Regex {
    Literal(String),
}

#[cfg_attr(test, derive(serde::Serialize))]
pub enum ParseError {
    InvalidRegex(String),
}

pub fn parse_regex(regex: &str) -> Result<Regex, ParseError> {
    if regex == "[" {
        Err(ParseError::InvalidRegex("Unmatched bracket".to_string()))
    } else {
        Ok(Regex::Literal(regex.to_string()))
    }
}

pub fn match_regex(regex: &Regex, input: &str) -> bool {
    match regex {
        Regex::Literal(literal) => input.contains(literal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blessed::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Deserialize)]
    struct Case {
        regex: String,
        inputs: Vec<String>,
    }

    #[derive(Serialize)]
    struct Output {
        parse_error: Option<ParseError>,
        ast: Option<Regex>,
        matches: HashMap<String, bool>,
    }

    #[blessed::harness]
    fn parse_compile_match(case: Case) -> Output {
        let parsed = parse_regex(&case.regex);
        match parsed {
            Ok(ast) => {
                let matches = case
                    .inputs
                    .iter()
                    .map(|input| (input.clone(), match_regex(&ast, input)))
                    .collect();
                Output {
                    ast: Some(ast),
                    parse_error: None,
                    matches,
                }
            }
            Err(e) => Output {
                ast: None,
                parse_error: Some(e),
                matches: HashMap::new(),
            },
        }
    }

    blessed::tests!("tests/*.blessed.json", "tests/blessed/");
}
