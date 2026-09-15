// Rust laid out the way rustfmt lays it out, covering the shapes the
// indenter has to get right. Formatted with `rustfmt --edition 2024`; the
// test types it back in with all of the indentation stripped off.
//
// The names are long on purpose: rustfmt joins anything that fits on one
// line, and it is the multi-line forms that are under test.
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ConfigurationForTheThing {
    pub name_of_the_configuration: String,
    pub number_of_retries_allowed: usize,
}

#[derive(Clone, Copy)]
pub enum KindOfChangeInTheDiff {
    SomethingWasAddedHere,
    SomethingWasRemovedHere,
}

pub struct EntryInTheMapping {
    pub key_of_the_entry: usize,
    pub value_of_the_entry: usize,
}

impl ConfigurationForTheThing {
    pub fn count_the_interesting_items(&self, items_to_look_through: &[usize]) -> usize {
        let total_of_every_item = items_to_look_through
            .iter()
            .copied()
            .filter(|item_being_examined| *item_being_examined > 0)
            .sum::<usize>();
        let scaled_total_of_every_item = total_of_every_item
            + self.number_of_retries_allowed
            + items_to_look_through.len()
            + self.name_of_the_configuration.len();
        let mapped_entries_by_key: BTreeMap<usize, usize> = items_to_look_through
            .iter()
            .map(|item_being_examined| EntryInTheMapping {
                key_of_the_entry: *item_being_examined,
                value_of_the_entry: *item_being_examined * 2,
            })
            .map(|entry| (entry.key_of_the_entry, entry.value_of_the_entry))
            .collect();
        let Some(first_item_in_the_list) =
            items_to_look_through.first().copied().filter(|i| *i > 0)
        else {
            return 0;
        };
        if scaled_total_of_every_item > first_item_in_the_list
            && mapped_entries_by_key.len() > 1
            && !self.name_of_the_configuration.is_empty()
        {
            for (key_of_the_entry, value_of_the_entry) in &mapped_entries_by_key {
                match KindOfChangeInTheDiff::SomethingWasAddedHere {
                    KindOfChangeInTheDiff::SomethingWasAddedHere => {
                        println!("{key_of_the_entry} {value_of_the_entry}");
                    }
                    KindOfChangeInTheDiff::SomethingWasRemovedHere => {}
                }
            }
        }
        scaled_total_of_every_item
    }

    pub fn describe_the_configuration_briefly(
        &self,
        prefix_for_the_description: &str,
        suffix_for_the_description: &str,
    ) -> String {
        format!(
            "{prefix_for_the_description}{}{suffix_for_the_description}",
            self.name_of_the_configuration,
        )
    }
}
