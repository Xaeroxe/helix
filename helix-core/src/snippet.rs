pub type Snippet<'a> = Vec<SnippetElement<'a>>;

#[derive(Debug, PartialEq, Eq)]
pub enum CaseChange {
    Upcase,
    Downcase,
    Capitalize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FormatItem<'a> {
    Text(&'a str),
    Capture(usize),
    CaseChange(usize, CaseChange),
    Conditional(usize, Option<&'a str>, Option<&'a str>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Regex<'a> {
    value: &'a str,
    replacement: Vec<FormatItem<'a>>,
    options: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SnippetElement<'a> {
    Tabstop {
        tabstop: usize,
    },
    Placeholder {
        tabstop: usize,
        value: Box<SnippetElement<'a>>,
    },
    Choice {
        tabstop: usize,
        choices: Vec<&'a str>,
    },
    Variable {
        name: &'a str,
        default: Option<&'a str>,
        regex: Option<Regex<'a>>,
    },
    Text(&'a str),
}

// TODO: remove this line once the parser is used.
#[allow(dead_code)]
mod parser {
    use once_cell::sync::Lazy;

    use crate::parser_combinator::*;

    use super::{CaseChange, FormatItem, Regex, Snippet, SnippetElement};

    /*
    https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#snippet_syntax

        any         ::= tabstop | placeholder | choice | variable | text
        tabstop     ::= '$' int | '${' int '}'
        placeholder ::= '${' int ':' any '}'
        choice      ::= '${' int '|' text (',' text)* '|}'
        variable    ::= '$' var | '${' var }'
                        | '${' var ':' any '}'
                        | '${' var '/' regex '/' (format | text)+ '/' options '}'
        format      ::= '$' int | '${' int '}'
                        | '${' int ':' '/upcase' | '/downcase' | '/capitalize' '}'
                        | '${' int ':+' if '}'
                        | '${' int ':?' if ':' else '}'
                        | '${' int ':-' else '}' | '${' int ':' else '}'
        regex       ::= Regular Expression value (ctor-string)
        options     ::= Regular Expression option (ctor-options)
        var         ::= [_a-zA-Z] [_a-zA-Z0-9]*
        int         ::= [0-9]+
        text        ::= .*
        if          ::= text
        else        ::= text
    */

    static DIGIT: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"^[0-9]+").unwrap());
    static VARIABLE: Lazy<regex::Regex> =
        Lazy::new(|| regex::Regex::new(r"^[_a-zA-Z][_a-zA-Z0-9]*").unwrap());
    static TEXT: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"^[^\$]+").unwrap());

    fn var<'a>() -> impl Parser<'a, Output = &'a str> {
        pattern(&VARIABLE)
    }

    fn digit<'a>() -> impl Parser<'a, Output = usize> {
        filter_map(pattern(&DIGIT), |s| s.parse().ok())
    }

    fn case_change<'a>() -> impl Parser<'a, Output = CaseChange> {
        use CaseChange::*;

        choice!(
            map("upcase", |_| Upcase),
            map("downcase", |_| Downcase),
            map("capitalize", |_| Capitalize),
        )
    }

    fn format<'a>() -> impl Parser<'a, Output = FormatItem<'a>> {
        use FormatItem::*;

        choice!(
            // '$' int
            map(right("$", digit()), Capture),
            // '${' int '}'
            map(seq!("${", digit(), "}"), |seq| Capture(seq.1)),
            // '${' int ':' '/upcase' | '/downcase' | '/capitalize' '}'
            map(seq!("${", digit(), ":/", case_change(), "}"), |seq| {
                CaseChange(seq.1, seq.3)
            }),
            // '${' int ':+' if '}'
            map(
                seq!("${", digit(), ":+", take_until(|c| c == '}'), "}"),
                |seq| { Conditional(seq.1, Some(seq.3), None) }
            ),
            // '${' int ':?' if ':' else '}'
            map(
                seq!(
                    "${",
                    digit(),
                    ":?",
                    take_until(|c| c == ':'),
                    ":",
                    take_until(|c| c == '}'),
                    "}"
                ),
                |seq| { Conditional(seq.1, Some(seq.3), Some(seq.5)) }
            ),
            // '${' int ':-' else '}' | '${' int ':' else '}'
            map(
                seq!(
                    "${",
                    digit(),
                    ":",
                    optional("-"),
                    take_until(|c| c == '}'),
                    "}"
                ),
                |seq| { Conditional(seq.1, None, Some(seq.4)) }
            ),
            // Any text
            map(pattern(&TEXT), Text),
        )
    }

    fn regex<'a>() -> impl Parser<'a, Output = Regex<'a>> {
        let replacement = reparse_as(take_until(|c| c == '/'), one_or_more(format()));

        map(
            seq!(
                "/",
                take_until(|c| c == '/'),
                "/",
                replacement,
                "/",
                optional(take_until(|c| c == '}')),
            ),
            |(_, value, _, replacement, _, options)| Regex {
                value,
                replacement,
                options,
            },
        )
    }

    fn tabstop<'a>() -> impl Parser<'a, Output = SnippetElement<'a>> {
        map(
            or(
                right("$", digit()),
                map(seq!("${", digit(), "}"), |values| values.1),
            ),
            |digit| SnippetElement::Tabstop { tabstop: digit },
        )
    }

    fn placeholder<'a>() -> impl Parser<'a, Output = SnippetElement<'a>> {
        // TODO: why doesn't parse_as work?
        // let value = reparse_as(take_until(|c| c == '}'), anything());
        let value = filter_map(take_until(|c| c == '}'), |s| {
            anything().parse(s).map(|parse_result| parse_result.1).ok()
        });

        map(seq!("${", digit(), ":", value, "}"), |seq| {
            SnippetElement::Placeholder {
                tabstop: seq.1,
                value: Box::new(seq.3),
            }
        })
    }

    fn choice<'a>() -> impl Parser<'a, Output = SnippetElement<'a>> {
        map(
            seq!(
                "${",
                digit(),
                "|",
                sep(take_until(|c| c == ',' || c == '|'), ","),
                "|}",
            ),
            |seq| SnippetElement::Choice {
                tabstop: seq.1,
                choices: seq.3,
            },
        )
    }

    fn variable<'a>() -> impl Parser<'a, Output = SnippetElement<'a>> {
        choice!(
            // $var
            map(right("$", var()), |name| SnippetElement::Variable {
                name,
                default: None,
                regex: None,
            }),
            // ${var:default}
            map(
                seq!("${", var(), ":", take_until(|c| c == '}'), "}",),
                |values| SnippetElement::Variable {
                    name: values.1,
                    default: Some(values.3),
                    regex: None,
                }
            ),
            // ${var/value/format/options}
            map(seq!("${", var(), regex(), "}"), |values| {
                SnippetElement::Variable {
                    name: values.1,
                    default: None,
                    regex: Some(values.2),
                }
            }),
        )
    }

    fn text<'a>() -> impl Parser<'a, Output = SnippetElement<'a>> {
        map(pattern(&TEXT), SnippetElement::Text)
    }

    fn anything<'a>() -> impl Parser<'a, Output = SnippetElement<'a>> {
        choice!(tabstop(), placeholder(), choice(), variable(), text())
    }

    fn snippet<'a>() -> impl Parser<'a, Output = Snippet<'a>> {
        one_or_more(anything())
    }

    pub fn parse(s: &str) -> Result<Snippet, &str> {
        snippet().parse(s).map(|(_input, elements)| elements)
    }

    #[cfg(test)]
    mod test {
        use super::SnippetElement::*;
        use super::*;

        #[test]
        fn empty_string_is_error() {
            assert_eq!(Err(""), parse(""));
        }

        #[test]
        fn parse_placeholders_in_function_call() {
            assert_eq!(
                Ok(vec![
                    Text("match("),
                    Placeholder {
                        tabstop: 1,
                        value: Box::new(Text("Arg1")),
                    },
                    Text(")")
                ]),
                parse("match(${1:Arg1})")
            )
        }

        #[test]
        fn parse_placeholders_in_statement() {
            assert_eq!(
                Ok(vec![
                    Text("local "),
                    Placeholder {
                        tabstop: 1,
                        value: Box::new(Text("var")),
                    },
                    Text(" = "),
                    Placeholder {
                        tabstop: 1,
                        value: Box::new(Text("value")),
                    },
                ]),
                parse("local ${1:var} = ${1:value}")
            )
        }

        #[test]
        fn parse_all() {
            assert_eq!(
                Ok(vec![
                    Text("hello "),
                    Tabstop { tabstop: 1 },
                    Tabstop { tabstop: 2 },
                    Text(" "),
                    Choice {
                        tabstop: 1,
                        choices: vec!["one", "two", "three"]
                    },
                    Text(" "),
                    Variable {
                        name: "name",
                        default: Some("foo"),
                        regex: None
                    },
                    Text(" "),
                    Variable {
                        name: "var",
                        default: None,
                        regex: None
                    },
                    Text(" "),
                    Variable {
                        name: "TM",
                        default: None,
                        regex: None
                    },
                ]),
                parse("hello $1${2} ${1|one,two,three|} ${name:foo} $var $TM")
            );
        }

        #[test]
        fn regex_capture_replace() {
            assert_eq!(
                Ok(vec![Variable {
                    name: "TM_FILENAME",
                    default: None,
                    regex: Some(Regex {
                        value: "(.*).+$",
                        replacement: vec![FormatItem::Capture(1)],
                        options: None,
                    }),
                }]),
                parse("${TM_FILENAME/(.*).+$/$1/}")
            );
        }
    }
}