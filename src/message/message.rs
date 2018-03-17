use std::mem::replace;

/// Maximum length of a serialized message in bytes
pub const MAX_LENGTH: usize = 512;

// A tag at the start of an IRC message
#[derive(PartialEq, Debug, Clone)]
pub struct MessageTag {
    pub name: String,
    pub value: Option<String>,
}

impl ToString for MessageTag {
    fn to_string(&self) -> String {
        match self.value {
            Some(ref value) => self.name.to_owned()+"="+&value,
            None => self.name.to_owned(),
        }
    }
}

// One IRC message, delimited by \r\n, or \n
#[derive(PartialEq, Debug, Clone)]
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

    /// If a message may have a very long trailing parameter, split it into multiple messages
    pub fn split_trailing_args(base_msg: Message, params: Vec<String>, separator: &str) -> Vec<Message> {
        let mut msgs = Vec::new();
        let base_len = base_msg.to_line().len();

        let max_param_len = MAX_LENGTH - base_len;
        let mut next_trailing = String::new();
        let mut params = params.into_iter().peekable();
        while let Some(param) = params.next() {
            let param_len = param.len() + separator.len();
            if !next_trailing.is_empty() && next_trailing.len() + param_len >= max_param_len {
                let mut next_msg = base_msg.clone();
                next_msg.params.push(replace(&mut next_trailing, String::new()));
                msgs.push(next_msg);
            }

            next_trailing += &param;
            if params.peek().is_some() {
                next_trailing += &separator;
            }
        }

        if !next_trailing.is_empty() {
            let mut next_msg = base_msg.clone();
            next_msg.params.push(next_trailing);
            msgs.push(next_msg);
        }

        msgs
    }

    pub fn to_line(&self) -> String {
        let mut line =  if self.tags.is_empty() {
            String::new()
        } else {
            "@".to_owned()+&self.tags.iter().map(|tag| tag.to_string()).collect::<Vec<_>>().join(";")+" "
        };

        if let Some(ref source) = self.source {
            line = line+":"+&source+" ";
        }

        // Empty command are a special case to get clean roundtrips on messages like ":only-a-source"
        if self.command.is_empty() {
            debug_assert!(self.params.is_empty());
            line.pop();
        } else {
            line += &self.command;

            for (i, param) in self.params.iter().enumerate() {
                if i == self.params.len() - 1 &&
                    (param.contains(" ") || param.contains(":") || param.is_empty()) {
                    line = line + " :" + param;
                } else {
                    debug_assert!(!param.contains(" "));
                    line = line + " " + param;
                }
            }
        }

        line+"\r\n"
    }

    fn consume_tags<'a>(msg_line: &'a str) -> (Vec<MessageTag>, &'a str) {
        assert!(msg_line.ends_with("\n"));
        let new_end = msg_line.len() - if msg_line.ends_with("\r\n") { 2 } else { 1 };
        let msg_line = msg_line[..new_end].trim_left();

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

    fn check(msg_line: &str, msg_is_normalized: bool, tags: &[(&str, Option<&str>)], source: Option<&str>, command: &str, params: &[&str]) {
        let msg_line = &(msg_line.to_owned()+"\r\n");
        let tags = tags.iter().map(|&(name, value)| MessageTag {
            name: name.to_string(),
            value: value.map(|s| s.to_string()),
        }).collect::<Vec<_>>();
        let parsed_msg = Message::new(msg_line);
        assert_eq!(parsed_msg.tags, tags);
        assert_eq!(parsed_msg.source, source.map(|s| s.to_string()));
        assert_eq!(parsed_msg.command, command.to_string());
        assert_eq!(parsed_msg.params, params);
        if msg_is_normalized {
            assert_eq!(&parsed_msg.to_line(), msg_line);
        }
    }

    // Test vectors happily copied from https://github.com/grawity/code/blob/3d9e1c43ef07671eda92289240ef7570d4e86b21/lib/tests/irc-split.txt

    #[test]
    fn good_line_endings() {
        let base = "foo bar baz";
        assert_eq!(Message::new(&(base.to_owned()+"\r\n")).to_line(), base.to_owned()+"\r\n");
        assert_eq!(Message::new(&(base.to_owned()+"\n")).to_line(), base.to_owned()+"\r\n");
    }

    #[test]
    #[should_panic]
    fn bad_line_ending() {
        Message::new("missing newline");
    }

    #[test]
    fn parse_without_command_params() {
        check("", true, &[], None, "", &[]);
        check(":", true, &[], Some(""), "", &[]);
        check(":bar", true, &[], Some("bar"), "", &[]);
        check("@baz", true, &[("baz", None)], None, "", &[]);
        check("@foo :bar", true, &[("foo", None)], Some("bar"), "", &[]);
    }

    #[test]
    fn parse_simple_commands_and_params() {
        check("foo", true, &[], None, "foo", &[]);
        check("foo bar", true, &[], None, "foo", &["bar"]);
        check("foo :bar", false, &[], None, "foo", &["bar"]);
        check("foo bar baz", true, &[], None, "foo", &["bar", "baz"]);
        check("foo :bar baz", true, &[], None, "foo", &["bar baz"]);
        check("foo bar :baz qux", true, &[], None, "foo", &["bar", "baz qux"]);
        check("Chin up! ::]", true, &[], None, "Chin", &["up!", ":]"]);
    }

    #[test]
    fn parse_prefixed() {
        check(":foo bar baz", true, &[], Some("foo"), "bar", &["baz"]);
        check(":foo bar :baz", false, &[], Some("foo"), "bar", &["baz"]);
        check(":foo bar :baz asdf", true, &[], Some("foo"), "bar", &["baz asdf"]);
        check(":foo bar :", true, &[], Some("foo"), "bar", &[""]);
        check(":foo bar :  ", true, &[], Some("foo"), "bar", &["  "]);
        check(":foo bar : baz asdf", true, &[], Some("foo"), "bar", &[" baz asdf"]);
    }

    #[test]
    fn parse_tagged() {
        check("@foo bar baz", true, &[("foo", None)], None, "bar", &["baz"]);
        check("@foo bar :baz", false, &[("foo", None)], None, "bar", &["baz"]);
        check("@foo bar :baz asdf", true, &[("foo", None)], None, "bar", &["baz asdf"]);
        check("@foo bar :", true, &[("foo", None)], None, "bar", &[""]);
        check("@foo bar :  ", true, &[("foo", None)], None, "bar", &["  "]);
        check("@foo bar : baz asdf", true, &[("foo", None)], None, "bar", &[" baz asdf"]);
    }

    #[test]
    fn parse_prefixed_and_tagged() {
        check("@foo :foo bar baz", true, &[("foo", None)], Some("foo"), "bar", &["baz"]);
        check("@foo :foo bar :baz", false, &[("foo", None)], Some("foo"), "bar", &["baz"]);
        check("@foo :foo bar :baz asdf", true, &[("foo", None)], Some("foo"), "bar", &["baz asdf"]);
        check("@foo :foo bar :", true, &[("foo", None)], Some("foo"), "bar", &[""]);
        check("@foo :foo bar :  ", true, &[("foo", None)], Some("foo"), "bar", &["  "]);
        check("@foo :foo bar : baz asdf", true, &[("foo", None)], Some("foo"), "bar", &[" baz asdf"]);
    }

    #[test]
    fn parse_tagged_with_values() {
        check("@foo bar baz", true, &[("foo", None)], None, "bar", &["baz"]);
        check("@foo= bar baz", true, &[("foo", Some(""))], None, "bar", &["baz"]);
        check("@foo=bar bar baz", true, &[("foo", Some("bar"))], None, "bar", &["baz"]);
        check("@foo=bar;baz=;qux bar baz", true, &[("foo", Some("bar")), ("baz", Some("")), ("qux", None)], None, "bar", &["baz"]);
        check("@baz;foo=bar;qux= bar baz", true, &[("baz", None), ("foo", Some("bar")), ("qux", Some(""))], None, "bar", &["baz"]);
    }

    #[test]
    fn parse_whitespace() {
        check(" foo bar baz", false, &[], None, "foo", &["bar", "baz"]);
        check(" :foo bar baz", false, &[], Some("foo"), "bar", &["baz"]);
        check(" @foo bar baz", false, &[("foo", None)], None, "bar", &["baz"]);
        check("foo   bar     baz   :asdf  ", false, &[], None, "foo", &["bar", "baz", "asdf  "]);
        check(":foo   bar     baz   :  asdf", false, &[], Some("foo"), "bar", &["baz", "  asdf"]);
        check("@foo   bar     baz   :  asdf", false, &[("foo", None)], None, "bar", &["baz", "  asdf"]);
        check("foo bar baz   ", false, &[], None, "foo", &["bar", "baz"]);
        check("foo bar :baz   ", true, &[], None, "foo", &["bar", "baz   "]);
        check("foo bar\tbaz asdf", true, &[], None, "foo", &["bar\tbaz", "asdf"]);
        check("foo bar :baz asdf\t", true, &[], None, "foo", &["bar", "baz asdf\t"]);
    }
}
