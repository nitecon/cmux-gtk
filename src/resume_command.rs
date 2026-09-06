//! Conservative literal shell argument parsing for reviewed command-prefix authority.

/// Parse a single literal POSIX-style command, rejecting expansion, control syntax and open quotes.
/// Exact-command approvals can still cover arbitrary shell scripts after explicit full-text review.
pub fn literal_arguments(command: &str) -> Option<Vec<String>> {
    if command.is_empty() || command.len() > 16384 {
        return None;
    }
    let mut arguments = Vec::new();
    let mut argument = String::new();
    let mut started = false;
    let mut quote = None;
    let mut characters = command.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\0' {
            return None;
        }
        match quote {
            Some('\'') => {
                if character == '\'' {
                    quote = None;
                } else {
                    argument.push(character);
                }
            }
            Some('"') => match character {
                '"' => quote = None,
                '$' | '`' | '!' | '\n' | '\r' => return None,
                '\\' => {
                    let next = characters.next()?;
                    if matches!(next, '\n' | '\r' | '\0') {
                        return None;
                    }
                    // POSIX double quotes preserve backslashes before ordinary characters.
                    if !matches!(next, '$' | '`' | '"' | '\\') {
                        argument.push('\\');
                    }
                    argument.push(next);
                }
                other => argument.push(other),
            },
            None => match character {
                '\'' | '"' => {
                    quote = Some(character);
                    started = true;
                }
                '\\' => {
                    let next = characters.next()?;
                    if matches!(next, '\n' | '\r' | '\0') {
                        return None;
                    }
                    argument.push(next);
                    started = true;
                }
                ' ' | '\t' => {
                    if started {
                        arguments.push(std::mem::take(&mut argument));
                        started = false;
                    }
                }
                '$' | '`' | '!' | '\n' | '\r' | '*' | '?' | '[' | '{' | '}' | ';' | '|' | '&'
                | '<' | '>' | '(' | ')' => return None,
                '~' | '=' | '#' if !started => return None,
                other => {
                    argument.push(other);
                    started = true;
                }
            },
            _ => unreachable!("only single and double quotes are stored"),
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        arguments.push(argument);
    }
    (!arguments.is_empty() && !arguments[0].is_empty()).then_some(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Token matching respects quotes and empty arguments, while rejecting shell execution escapes.
    #[test]
    fn literal_prefix_arguments() {
        assert_eq!(
            literal_arguments("'/opt/agent cli' --resume 'id '\\''quoted' ''").unwrap(),
            ["/opt/agent cli", "--resume", "id 'quoted", ""]
        );
        assert_eq!(
            literal_arguments(r#"agent "a\q" '\$HOME'"#).unwrap(),
            ["agent", r"a\q", r"\$HOME"]
        );
        for unsafe_command in [
            "agent x; true",
            "agent x|true",
            "agent $(true)",
            "agent \"$HOME\"",
            "agent `true`",
            "agent x\ntrue",
            "agent *",
            "agent ~/x",
            "agent #comment",
            "agent 'open",
            "agent \\",
            "agent \\\ntrue",
        ] {
            assert!(
                literal_arguments(unsafe_command).is_none(),
                "{unsafe_command:?}"
            );
        }
    }
}
