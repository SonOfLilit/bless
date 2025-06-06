#[cfg_attr(test, derive(serde::Serialize, Debug))]
pub enum Regex {
    Literal(String),
    CharClass(String),
}

#[cfg_attr(test, derive(serde::Serialize))]
pub enum ParseError {
    InvalidRegex(String),
}

pub fn parse_regex(regex: &str) -> Result<Regex, ParseError> {
    if regex.starts_with('[') && regex.ends_with(']') && regex.len() >= 2 {
        let chars = regex[1..regex.len() - 1].to_string();
        if chars.contains('[') || chars.contains(']') {
             Err(ParseError::InvalidRegex("Nested or mismatched brackets not supported".to_string()))
        } else {
             Ok(Regex::CharClass(chars))
        }
    } else if regex.contains('[') || regex.contains(']') {
        Err(ParseError::InvalidRegex("Mismatched or misplaced brackets".to_string()))
    }
     else {
        Ok(Regex::Literal(regex.to_string()))
    }
}

pub fn match_regex(regex: &Regex, input: &str) -> bool {
    match regex {
        Regex::Literal(literal) => input.contains(literal),
        Regex::CharClass(chars) => input.chars().any(|c| chars.contains(c)),
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

    blessed::tests!();
}
