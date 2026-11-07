//! First-launch setup flow state. Pure: no UI or platform code.

/// One screen of the setup, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Model,
    HowItWorks,
    MakeItYours,
    AllSet,
}

const STEPS: [Step; Onboarding::COUNT] = [
    Step::Welcome,
    Step::Model,
    Step::HowItWorks,
    Step::MakeItYours,
    Step::AllSet,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Onboarding {
    index: usize,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::new()
    }
}

impl Onboarding {
    pub const COUNT: usize = 5;

    pub fn new() -> Self {
        Self { index: 0 }
    }

    pub fn step(&self) -> Step {
        STEPS[self.index]
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn next(&mut self) {
        self.index = (self.index + 1).min(Self::COUNT - 1);
    }

    pub fn back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn is_first(&self) -> bool {
        self.index == 0
    }

    pub fn is_last(&self) -> bool {
        self.index == Self::COUNT - 1
    }
}

/// Result of looking for a local model server during setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Checking,
    Found(Vec<String>),
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_saturates() {
        let mut o = Onboarding::new();
        assert_eq!(o.step(), Step::Welcome);
        assert!(o.is_first());
        o.back();
        assert_eq!(o.step(), Step::Welcome);
        for _ in 0..4 {
            o.next();
        }
        assert_eq!(o.step(), Step::AllSet);
        assert!(o.is_last());
        assert_eq!(o.index(), 4);
        o.next();
        assert_eq!(o.step(), Step::AllSet);
        o.back();
        assert_eq!(o.step(), Step::MakeItYours);
    }
}
