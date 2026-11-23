use std::f32::consts::{FRAC_PI_2, TAU};

use crate::convert::{Choice, Format, Group};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Category(Group),
    Output(Format),
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Self::Category(group) => group.label(),
            Self::Output(format) => format.label(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Slice {
    item: Item,
    pub inner: f32,
    pub outer: f32,
    pub angle: f32,
    pub half_width: f32,
}

impl Slice {
    pub fn label(self) -> &'static str {
        self.item.label()
    }
}

#[derive(Default)]
pub struct Menu {
    choices: Vec<Choice>,
    groups: Vec<Group>,
    group: Option<Group>,
    hovered: Option<Item>,
    reason: Option<&'static str>,
}

impl Menu {
    pub fn set_choices(&mut self, choices: Vec<Choice>, reason: Option<&'static str>) {
        *self = Self {
            choices,
            reason,
            ..Self::default()
        };
        for choice in &self.choices {
            if !self.groups.contains(&choice.group) {
                self.groups.push(choice.group);
            }
        }
    }

    pub fn title(&self) -> &'static str {
        if let Some(reason) = self.reason {
            return reason;
        }
        if self.choices.is_empty() {
            return "Kéo tệp vào đây";
        }
        self.hovered
            .or(self.group.map(Item::Category))
            .map_or("Chuyển đổi", Item::label)
    }

    pub fn selected(&self, slice: Slice) -> bool {
        self.hovered == Some(slice.item) || self.group.map(Item::Category) == Some(slice.item)
    }

    pub fn slices(&self) -> Vec<Slice> {
        if self.groups.len() <= 1 {
            return ring(
                self.choices
                    .iter()
                    .map(|choice| Item::Output(choice.format)),
                0.40,
                0.92,
            );
        }
        let categories = self.groups.iter().copied().map(Item::Category);
        if self.group.is_none() {
            return ring(categories, 0.40, 0.92);
        }
        let mut slices = ring(categories, 0.40, 0.62);
        let formats = self
            .choices
            .iter()
            .filter(|choice| Some(choice.group) == self.group)
            .map(|choice| Item::Output(choice.format));
        slices.extend(ring(formats, 0.65, 0.92));
        slices
    }

    pub fn point(&mut self, x: f32, y: f32) -> bool {
        let previous = (self.group, self.hovered);
        let radius = x.hypot(y);
        let angle = y.atan2(x);
        let item = self
            .slices()
            .into_iter()
            .find(|slice| {
                let diff = (angle - slice.angle + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0;
                (slice.inner..=slice.outer).contains(&radius) && diff.abs() <= slice.half_width
            })
            .map(|slice| slice.item);
        if let Some(Item::Category(group)) = item {
            self.group = Some(group);
        } else if radius < 0.36 {
            self.group = None;
        }
        self.hovered = item;
        previous != (self.group, self.hovered)
    }

    pub fn output(&self) -> Option<Format> {
        match self.hovered {
            Some(Item::Output(format)) => Some(format),
            _ => None,
        }
    }
}

fn ring(items: impl Iterator<Item = Item>, inner: f32, outer: f32) -> Vec<Slice> {
    let items: Vec<_> = items.collect();
    if items.is_empty() {
        return Vec::new();
    }
    let step = TAU / items.len() as f32;
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| Slice {
            item,
            inner,
            outer,
            angle: index as f32 * step - FRAC_PI_2,
            half_width: step / 2.0 - 0.035,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_visible_sectors_accept_output() {
        let mut menu = Menu::default();
        menu.set_choices(
            vec![
                Choice {
                    group: Group::Image,
                    format: Format::Jpg,
                },
                Choice {
                    group: Group::Image,
                    format: Format::Png,
                },
            ],
            None,
        );
        for (x, y, output) in [
            (0.0, 0.0, None),
            (0.0, -0.7, Some(Format::Jpg)),
            (0.0, 0.7, Some(Format::Png)),
            (1.0, 0.0, None),
            (0.85, 0.0, None),
        ] {
            menu.point(x, y);
            assert_eq!(menu.output(), output);
        }
    }

    #[test]
    fn moving_from_a_group_to_the_outer_ring_selects_an_output() {
        let mut menu = Menu::default();
        menu.set_choices(
            vec![
                Choice {
                    group: Group::Video,
                    format: Format::Webm,
                },
                Choice {
                    group: Group::Audio,
                    format: Format::Mp3,
                },
            ],
            None,
        );
        menu.point(0.0, -0.50);
        assert_eq!(menu.output(), None);
        assert_eq!(menu.title(), "Video");
        menu.point(0.0, -0.75);
        assert_eq!(menu.output(), Some(Format::Webm));
        menu.point(0.0, 0.50);
        assert_eq!(menu.title(), "Âm thanh");
        menu.point(0.0, -0.75);
        assert_eq!(menu.output(), Some(Format::Mp3));
        menu.point(0.0, 0.0);
        assert_eq!(menu.output(), None);
        assert_eq!(menu.title(), "Chuyển đổi");
    }
}
