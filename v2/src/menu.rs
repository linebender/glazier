use std::borrow::Cow;

/// An abstract menu object
///
/// These have value semantics - that is
pub struct MenuBuilder<'a> {
    items: Vec<MenuMember<'a>>,
}

impl<'a> MenuBuilder<'a> {
    pub fn add_child(&mut self) {}

    pub fn with_child_with_children(&mut self) {}

    pub fn with_child_with_children_builder(&mut self) {}
}

// Kinds of menu item:
// - Normal (e.g. 'Copy/Paste')
// - Break/seperator (e.g. -------). These can't have children (?)
// -

pub enum MenuItem {
    Ordinary,
    Break,
}

pub enum Command {
    Copy,
    Paste,
    Undo,
    Redo,
    Custom(u32),
}

#[non_exhaustive]
pub struct MenuItems<'a> {
    pub display: Cow<'a, str>,
}

pub(crate) struct MenuIterator<'a, 'b> {
    items: &'a [MenuMember<'b>],
    stack: Vec<MenuItemId>,
}

impl<'a, 'b> Iterator for MenuIterator<'a, 'b> {
    type Item = &'a MenuItem<'b>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.stack.last().copied()?;
        let current_item = &self.items[current.0];
        if let Some(child) = current_item.first_child {
            self.stack.push(child);
        } else if let Some(sibling) = current_item.next_sibling {
            self.stack.pop();
            self.stack.push(sibling);
        } else {
            self.stack.pop();
        }
        Some(&current_item.item)
    }
}

struct MenuMember<'a> {
    item: MenuItem<'a>,
    next_sibling: Option<MenuItemId>,
    first_child: Option<MenuItemId>,
}

#[derive(Clone, Copy)]
pub struct MenuItemId(usize);

/// Allows building an application menu which matches platform conventions
pub struct AppMenuBuilder {}
