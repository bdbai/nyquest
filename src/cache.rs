#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachingBehavior {
    Disabled,
    GoodToHave,
    #[default]
    Default,
}
