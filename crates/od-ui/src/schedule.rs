//! Automation schedule presets (the original's Hourly / Daily / Weekdays /
//! Weekly) as the cron strings the daemon stores: `CRON_TZ=<zone> m h * * d`
//! (no zone for hourly), plus a custom cron expression.

/// A schedule as the form edits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preset {
    Hourly {
        minute: u32,
    },
    Daily {
        hour: u32,
        minute: u32,
        tz: String,
    },
    Weekdays {
        hour: u32,
        minute: u32,
        tz: String,
    },
    /// `weekday`: 0 = Sunday … 6 = Saturday.
    Weekly {
        weekday: u32,
        hour: u32,
        minute: u32,
        tz: String,
    },
    Custom(String),
}

/// The preset kinds, in the picker's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Hourly,
    Daily,
    Weekdays,
    Weekly,
    Custom,
}

impl Kind {
    pub const ALL: [Self; 5] = [
        Self::Hourly,
        Self::Daily,
        Self::Weekdays,
        Self::Weekly,
        Self::Custom,
    ];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekdays => "weekdays",
            Self::Weekly => "weekly",
            Self::Custom => "custom",
        }
    }
}

/// "HH:MM" → (hour, minute).
#[must_use]
pub fn parse_time(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

#[must_use]
pub fn format_time(hour: u32, minute: u32) -> String {
    format!("{hour:02}:{minute:02}")
}

impl Preset {
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Hourly { .. } => Kind::Hourly,
            Self::Daily { .. } => Kind::Daily,
            Self::Weekdays { .. } => Kind::Weekdays,
            Self::Weekly { .. } => Kind::Weekly,
            Self::Custom(_) => Kind::Custom,
        }
    }

    /// The stored schedule string.
    #[must_use]
    pub fn to_cron(&self) -> String {
        let zoned = |tz: &str, fields: String| {
            if tz.trim().is_empty() {
                fields
            } else {
                format!("CRON_TZ={} {fields}", tz.trim())
            }
        };
        match self {
            Self::Hourly { minute } => format!("{minute} * * * *"),
            Self::Daily { hour, minute, tz } => zoned(tz, format!("{minute} {hour} * * *")),
            Self::Weekdays { hour, minute, tz } => zoned(tz, format!("{minute} {hour} * * 1-5")),
            Self::Weekly {
                weekday,
                hour,
                minute,
                tz,
            } => zoned(tz, format!("{minute} {hour} * * {weekday}")),
            Self::Custom(raw) => raw.trim().to_owned(),
        }
    }

    /// Reads a stored schedule back into a preset; anything the presets do
    /// not produce is [`Preset::Custom`].
    #[must_use]
    pub fn from_cron(raw: &str) -> Self {
        let raw = raw.trim();
        let (tz, fields) = match raw.strip_prefix("CRON_TZ=") {
            Some(rest) => match rest.split_once(' ') {
                Some((tz, f)) => (tz.to_owned(), f.trim()),
                None => return Self::Custom(raw.to_owned()),
            },
            None => (String::new(), raw),
        };
        let parts: Vec<&str> = fields.split_whitespace().collect();
        let num = |s: &str, max: u32| s.parse::<u32>().ok().filter(|v| *v <= max);
        let custom = || Self::Custom(raw.to_owned());
        let [mi, h, dom, mo, dow] = parts[..] else {
            return custom();
        };
        if dom != "*" || mo != "*" {
            return custom();
        }
        let Some(minute) = num(mi, 59) else {
            return custom();
        };
        if h == "*" && dow == "*" && tz.is_empty() {
            return Self::Hourly { minute };
        }
        let Some(hour) = num(h, 23) else {
            return custom();
        };
        match dow {
            "*" => Self::Daily { hour, minute, tz },
            "1-5" => Self::Weekdays { hour, minute, tz },
            d => match num(d, 6) {
                Some(weekday) => Self::Weekly {
                    weekday,
                    hour,
                    minute,
                    tz,
                },
                None => custom(),
            },
        }
    }

    /// The same schedule as another kind, keeping time and zone.
    #[must_use]
    pub fn with_kind(&self, kind: Kind, local_tz: &str) -> Self {
        let (hour, minute, tz, weekday) = match self {
            Self::Hourly { minute } => (9, *minute, local_tz.to_owned(), 1),
            Self::Daily { hour, minute, tz } | Self::Weekdays { hour, minute, tz } => {
                (*hour, *minute, tz.clone(), 1)
            }
            Self::Weekly {
                weekday,
                hour,
                minute,
                tz,
            } => (*hour, *minute, tz.clone(), *weekday),
            Self::Custom(_) => (9, 0, local_tz.to_owned(), 1),
        };
        match kind {
            Kind::Hourly => Self::Hourly { minute },
            Kind::Daily => Self::Daily { hour, minute, tz },
            Kind::Weekdays => Self::Weekdays { hour, minute, tz },
            Kind::Weekly => Self::Weekly {
                weekday,
                hour,
                minute,
                tz,
            },
            Kind::Custom => Self::Custom(self.to_cron()),
        }
    }
}

/// The city part of an IANA zone: `America/New_York` → `New York`.
#[must_use]
pub fn city(tz: &str) -> String {
    tz.rsplit('/').next().unwrap_or(tz).replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_through_cron() {
        let cases = [
            (Preset::Hourly { minute: 15 }, "15 * * * *"),
            (
                Preset::Daily {
                    hour: 9,
                    minute: 0,
                    tz: "Asia/Tokyo".into(),
                },
                "CRON_TZ=Asia/Tokyo 0 9 * * *",
            ),
            (
                Preset::Weekdays {
                    hour: 18,
                    minute: 30,
                    tz: String::new(),
                },
                "30 18 * * 1-5",
            ),
            (
                Preset::Weekly {
                    weekday: 0,
                    hour: 7,
                    minute: 5,
                    tz: "UTC".into(),
                },
                "CRON_TZ=UTC 5 7 * * 0",
            ),
        ];
        for (preset, cron) in cases {
            assert_eq!(preset.to_cron(), cron);
            assert_eq!(Preset::from_cron(cron), preset, "{cron}");
        }
        assert_eq!(
            Preset::from_cron("*/15 9-17 * * *"),
            Preset::Custom("*/15 9-17 * * *".into())
        );
        assert_eq!(Preset::from_cron("@daily"), Preset::Custom("@daily".into()));
    }

    #[test]
    fn switching_kind_keeps_the_time() {
        let daily = Preset::Daily {
            hour: 8,
            minute: 45,
            tz: "Europe/Berlin".into(),
        };
        assert_eq!(
            daily.with_kind(Kind::Weekly, "UTC"),
            Preset::Weekly {
                weekday: 1,
                hour: 8,
                minute: 45,
                tz: "Europe/Berlin".into()
            }
        );
        assert_eq!(
            daily.with_kind(Kind::Hourly, "UTC"),
            Preset::Hourly { minute: 45 }
        );
        assert_eq!(parse_time("07:05"), Some((7, 5)));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(city("America/New_York"), "New York");
    }
}
