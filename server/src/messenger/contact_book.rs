use gg_protocol::{
  GGNumber,
  packets::{ContactEntry, ContactType},
};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct ContactBook(HashMap<GGNumber, u8>);
impl ContactBook {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn is_blocked(&self, uin: GGNumber) -> bool {
    self.0.get(&uin).is_some_and(|flags| flags & 4 != 0)
  }
  pub fn is_friend(&self, uin: GGNumber) -> bool {
    self.0.get(&uin).is_some_and(|flags| flags & 2 != 0)
  }
  pub fn is_hidden_from(&self, uin: GGNumber) -> bool {
    self
      .0
      .get(&uin)
      .is_some_and(|flags| flags & 4 != 0 || flags & 3 == 1)
  }
  pub fn watched(&self) -> Vec<GGNumber> {
    self
      .0
      .iter()
      .filter(|(_, flags)| **flags & 4 == 0)
      .map(|(uin, _)| *uin)
      .collect()
  }
  pub fn entries(&self) -> Vec<ContactEntry> {
    self
      .0
      .iter()
      .map(|(uin, flags)| ContactEntry {
        uin: *uin,
        user_type: ContactType::try_from(*flags).unwrap(),
      })
      .collect()
  }
  pub fn set(&mut self, contacts: &[ContactEntry]) {
    self.0.clear();
    for contact in contacts {
      self.add(contact);
    }
  }
  pub fn add(&mut self, contact: &ContactEntry) {
    *self.0.entry(contact.uin).or_default() |= contact.user_type.bits();
  }
  pub fn remove(&mut self, contact: &ContactEntry) {
    if let Some(flags) = self.0.get_mut(&contact.uin) {
      *flags &= !contact.user_type.bits();
      if *flags == 0 {
        self.0.remove(&contact.uin);
      }
    }
  }
}
