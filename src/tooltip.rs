use crate::Icon;

/// Rich tooltip content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tooltip {
    pub title: String,
    pub body: String,
    pub icon: Option<Icon>,
}

impl Tooltip {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            icon: None,
        }
    }
}
