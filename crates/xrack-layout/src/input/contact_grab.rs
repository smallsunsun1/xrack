/// Recognition state for a physical contact or contact group. A passive
/// observer can watch a sequence until a recognizer claims it exclusively.
#[derive(Clone, Debug)]
pub(super) struct ContactGrab<Observer, Owner> {
    passive: Vec<Observer>,
    pub(super) exclusive: Option<Owner>,
}

impl<Observer, Owner> Default for ContactGrab<Observer, Owner> {
    fn default() -> Self {
        Self {
            passive: Vec::new(),
            exclusive: None,
        }
    }
}

impl<Observer, Owner> ContactGrab<Observer, Owner> {
    pub(super) fn observe(&mut self, observer: Observer) {
        if self.exclusive.is_none() {
            self.passive.push(observer);
        }
    }

    pub(super) fn first_observer(&self) -> Option<&Observer> {
        self.passive.first()
    }

    pub(super) fn first_observer_mut(&mut self) -> Option<&mut Observer> {
        self.passive.first_mut()
    }

    pub(super) fn remove_observers(&mut self, matches: impl FnMut(&Observer) -> bool) {
        let mut matches = matches;
        self.passive.retain(|observer| !matches(observer));
    }

    pub(super) fn take_tap(&mut self) -> Option<Observer> {
        if self.exclusive.is_some() {
            return None;
        }
        self.passive.pop()
    }

    /// Return displaced observers and the previous owner for cancellation.
    pub(super) fn claim(&mut self, owner: Owner) -> (Vec<Observer>, Option<Owner>) {
        (
            std::mem::take(&mut self.passive),
            self.exclusive.replace(owner),
        )
    }

    pub(super) fn cancel(&mut self) -> (Vec<Observer>, Option<Owner>) {
        (std::mem::take(&mut self.passive), self.exclusive.take())
    }
}

#[cfg(test)]
mod tests {
    use super::ContactGrab;

    #[test]
    fn exclusive_claim_cancels_all_passive_observers_and_transfers_ownership() {
        let mut grab = ContactGrab::default();
        grab.observe("button");
        grab.observe("pointer handler");
        assert_eq!(
            grab.claim("scroll"),
            (vec!["button", "pointer handler"], None)
        );
        grab.observe("late observer");
        assert_eq!(grab.take_tap(), None);
        assert_eq!(grab.claim("pinch"), (vec![], Some("scroll")));
        assert_eq!(grab.cancel(), (vec![], Some("pinch")));
        assert_eq!(grab.cancel(), (vec![], None));
    }
}
