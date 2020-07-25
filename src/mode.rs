pub trait BaseMode: ToString {
    fn get_mode_bool(&mut self, mode: u8) -> Option<&mut bool>;

    /// Return the applied modestring no matter what, but signals error on unknown modes
    fn apply_modestring(&mut self, modestring: &str) -> Result<String, (String, char)> {
        let mut applied_modestring = String::new();
        if modestring.is_empty() {
            return Ok(applied_modestring);
        }

        let mut had_unknown_mode = false;
        let mut unknown_mode = '\0';

        let mut positive = true;
        let mut last_positive_applied = positive;
        for &c in modestring.as_bytes() {
            match c {
                b'+' => positive = true,
                b'-' => positive = false,
                _ => {
                    let result = self.apply_mode(c, positive, &mut last_positive_applied, &mut applied_modestring);
                    if result.is_err() {
                        had_unknown_mode = true;
                        unknown_mode = c as char;
                    }
                },
            }
        }

        if had_unknown_mode {
            Err((applied_modestring, unknown_mode))
        } else {
            Ok(applied_modestring)
        }
    }

    /// Updates self and the modestring, if there was a change
    fn apply_mode(&mut self, mode: u8, positive: bool, last_positive_applied: &mut bool, modestring: &mut String) -> Result<(), ()> {
        let target = match self.get_mode_bool(mode) {
            Some(target) => target,
            None => return Err(()),
        };

        if *target != positive {
            *target = positive;
            let positive_changed = modestring.is_empty() || positive != *last_positive_applied;
            Self::append_mode(modestring, mode, positive_changed, positive);
            *last_positive_applied = positive;
        }

        Ok(())
    }

    fn append_mode(modestring: &mut String, mode: u8, positive_changed: bool, positive: bool) {
        if positive_changed && positive {
            modestring.push('+');
        } else if positive_changed && !positive {
            modestring.push('-');
        }
        modestring.push(mode as char);
    }
}

pub struct UserMode {
    pub invisible: bool,
    pub see_wallops: bool,
    pub is_bot: bool,
}

impl Default for UserMode {
    fn default() -> Self {
        Self {
            invisible: true,
            see_wallops: false,
            is_bot: false,
        }
    }
}

impl BaseMode for UserMode {
    fn get_mode_bool(&mut self, mode: u8) -> Option<&mut bool> {
        Some(match mode {
            b'i' => &mut self.invisible,
            b'w' => &mut self.see_wallops,
            b'B' => &mut self.is_bot,
            _ => return None,
        })
    }
}

impl ToString for UserMode {
    fn to_string(&self) -> String {
        let mut modestring = "+".to_owned();
        if self.invisible { modestring.push('i'); }
        if self.see_wallops { modestring.push('w'); }
        if self.is_bot { modestring.push('B'); }

        modestring
    }
}

/// NOTE: Don't forget to update CHANMODES when adding a new mode!
pub const CHANMODES: &str = ",,,n";

pub struct ChannelMode {
    pub no_external_msgs: bool,
}

impl Default for ChannelMode {
    fn default() -> Self {
        Self {
            no_external_msgs: true,
        }
    }
}

impl ToString for ChannelMode {
    fn to_string(&self) -> String {
        let mut modestring = "+".to_owned();
        if self.no_external_msgs { modestring.push('n'); }

        modestring
    }
}

impl BaseMode for ChannelMode {
    fn get_mode_bool(&mut self, mode: u8) -> Option<&mut bool> {
        Some(match mode {
            b'n' => &mut self.no_external_msgs,
            _ => return None,
        })
    }
}
