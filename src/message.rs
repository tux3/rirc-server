// A tag at the start of an IRC message
#[derive(PartialEq, Debug)]
pub struct MessageTag {
    pub name: String,
    pub value: Option<String>,
}

// One IRC message, delimited by \r\n, or \n
#[derive(PartialEq, Debug)]
pub struct Message {
    pub tags: Vec<MessageTag>,
    pub source: Option<String>,
    pub command: String,
    pub params: Vec<String>,
}

impl Message {
    pub fn new(msg_line: &str) -> Message {
        let (tags, msg_line) = Message::consume_tags(msg_line);
        let (source, msg_line) = Message::consume_source(msg_line);
        let (command, params) = Message::parse_command_params(msg_line);

        Message {
            tags,
            source,
            command,
            params,
        }
    }

    fn consume_tags<'a>(msg_line: &'a str) -> (Vec<MessageTag>, &'a str) {
        let msg_line = msg_line.trim_left();
        if msg_line.bytes().next() == Some('@' as u8) {
            let (tags_word, next_msg_line) = if let Some(next_space) = msg_line.find(' ') {
                (&msg_line[1..next_space], &msg_line[next_space..])
            } else {
                (&msg_line[1..], "")
            };

            let tags = tags_word.split(";").map(|tag| {
                if let Some(equal) = tag.find('=') {
                    MessageTag{
                        name: tag[..equal].to_string(),
                        value: Some(tag[equal+1..].to_string()),
                    }
                } else {
                    MessageTag{
                        name: tag.to_string(),
                        value: None,
                    }
                }
            }).collect();
            (tags, next_msg_line)
        } else {
            (Vec::new(), msg_line)
        }
    }

    fn consume_source<'a>(msg_line: &'a str) -> (Option<String>, &'a str) {
        let msg_line = msg_line.trim_left();
        if msg_line.bytes().next() == Some(':' as u8) {
            match msg_line.find(' ') {
                Some(next_space) => (Some(msg_line[1..next_space].to_string()), &msg_line[next_space..]),
                None => (Some(msg_line[1..].to_string()), ""),
            }
        } else {
            (None, msg_line)
        }
    }

    fn parse_command_params(msg_line: &str) -> (String, Vec<String>) {
        let words = &mut msg_line.trim_left().split(' ');
        let command = words.next().unwrap_or("").to_string();
        let mut params = Vec::new();
        loop {
            let param = match words.next() {
                Some(word) => word,
                None => return (command, params),
            };
            if param.bytes().next() == Some(':' as u8) {
                params.push(words.fold(param[1..].to_string(), |s, w| s+" "+w));
            } else if !param.is_empty() {
                params.push(param.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(msg_line: &str, tags: &[(&str, Option<&str>)], source: Option<&str>, command: &str, params: &[&str]) {
        let tags = tags.iter().map(|&(name, value)| MessageTag {
            name: name.to_string(),
            value: value.map(|s| s.to_string()),
        }).collect::<Vec<_>>();
        let parsed_msg = Message::new(msg_line);
        assert_eq!(parsed_msg.tags, tags);
        assert_eq!(parsed_msg.source, source.map(|s| s.to_string()));
        assert_eq!(parsed_msg.command, command.to_string());
        assert_eq!(parsed_msg.params, params);
    }

    // Test vectors happily copied from https://github.com/grawity/code/blob/3d9e1c43ef07671eda92289240ef7570d4e86b21/lib/tests/irc-split.txt

    #[test]
    fn parse_without_command_params() {
        check("", &[], None, "", &[]);
        check(":", &[], Some(""), "", &[]);
        check(":bar", &[], Some("bar"), "", &[]);
        check("@baz", &[("baz", None)], None, "", &[]);
        check("@foo :bar", &[("foo", None)], Some("bar"), "", &[]);
    }

    #[test]
    fn parse_simple_commands_and_params() {
        check("foo", &[], None, "foo", &[]);
        check("foo bar", &[], None, "foo", &["bar"]);
        check("foo :bar", &[], None, "foo", &["bar"]);
        check("foo bar baz", &[], None, "foo", &["bar", "baz"]);
        check("foo :bar baz", &[], None, "foo", &["bar baz"]);
        check("foo bar :baz qux", &[], None, "foo", &["bar", "baz qux"]);
    }

    #[test]
    fn parse_prefixed() {
        check(":foo bar baz", &[], Some("foo"), "bar", &["baz"]);
        check(":foo bar :baz", &[], Some("foo"), "bar", &["baz"]);
        check(":foo bar :baz asdf", &[], Some("foo"), "bar", &["baz asdf"]);
        check(":foo bar :", &[], Some("foo"), "bar", &[""]);
        check(":foo bar :  ", &[], Some("foo"), "bar", &["  "]);
        check(":foo bar : baz asdf", &[], Some("foo"), "bar", &[" baz asdf"]);
    }

    #[test]
    fn parse_tagged() {
        check("@foo bar baz", &[("foo", None)], None, "bar", &["baz"]);
        check("@foo bar :baz", &[("foo", None)], None, "bar", &["baz"]);
        check("@foo bar :baz asdf", &[("foo", None)], None, "bar", &["baz asdf"]);
        check("@foo bar :", &[("foo", None)], None, "bar", &[""]);
        check("@foo bar :  ", &[("foo", None)], None, "bar", &["  "]);
        check("@foo bar : baz asdf", &[("foo", None)], None, "bar", &[" baz asdf"]);
    }

    #[test]
    fn parse_prefixed_and_tagged() {
        check("@foo :foo bar baz", &[("foo", None)], Some("foo"), "bar", &["baz"]);
        check("@foo :foo bar :baz", &[("foo", None)], Some("foo"), "bar", &["baz"]);
        check("@foo :foo bar :baz asdf", &[("foo", None)], Some("foo"), "bar", &["baz asdf"]);
        check("@foo :foo bar :", &[("foo", None)], Some("foo"), "bar", &[""]);
        check("@foo :foo bar :  ", &[("foo", None)], Some("foo"), "bar", &["  "]);
        check("@foo :foo bar : baz asdf", &[("foo", None)], Some("foo"), "bar", &[" baz asdf"]);
    }

    #[test]
    fn parse_tagged_with_values() {
        check("@foo bar baz", &[("foo", None)], None, "bar", &["baz"]);
        check("@foo= bar baz", &[("foo", Some(""))], None, "bar", &["baz"]);
        check("@foo=bar bar baz", &[("foo", Some("bar"))], None, "bar", &["baz"]);
        check("@foo=bar;baz=;qux bar baz", &[("foo", Some("bar")), ("baz", Some("")), ("qux", None)], None, "bar", &["baz"]);
        check("@baz;foo=bar;qux= bar baz", &[("baz", None), ("foo", Some("bar")), ("qux", Some(""))], None, "bar", &["baz"]);
    }

    #[test]
    fn parse_whitespace() {
        check(" foo bar baz", &[], None, "foo", &["bar", "baz"]);
        check(" :foo bar baz", &[], Some("foo"), "bar", &["baz"]);
        check(" @foo bar baz", &[("foo", None)], None, "bar", &["baz"]);
        check("foo   bar     baz   :asdf  ", &[], None, "foo", &["bar", "baz", "asdf  "]);
        check(":foo   bar     baz   :  asdf", &[], Some("foo"), "bar", &["baz", "  asdf"]);
        check("@foo   bar     baz   :  asdf", &[("foo", None)], None, "bar", &["baz", "  asdf"]);
        check("foo bar baz   ", &[], None, "foo", &["bar", "baz"]);
        check("foo bar :baz   ", &[], None, "foo", &["bar", "baz   "]);
        check("foo bar\tbaz asdf", &[], None, "foo", &["bar\tbaz", "asdf"]);
        check("foo bar :baz asdf\t", &[], None, "foo", &["bar", "baz asdf\t"]);
    }
}
