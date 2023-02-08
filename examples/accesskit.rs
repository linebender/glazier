// Copyright 2022 The Druid Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{any::Any, num::NonZeroU128};

use accesskit::{
    Action, ActionRequest, CheckedState, DefaultActionVerb, Node, NodeBuilder, NodeClassSet,
    NodeId, Rect, Role, Tree, TreeUpdate,
};

use glazier::kurbo::Size;

use glazier::{Application, KbKey, KeyEvent, Region, WinHandler, WindowBuilder, WindowHandle};

const WINDOW_TITLE: &str = "Hello world";

const WINDOW_ID: NodeId = NodeId(unsafe { NonZeroU128::new_unchecked(1) });
const CHECKBOX_1_ID: NodeId = NodeId(unsafe { NonZeroU128::new_unchecked(2) });
const CHECKBOX_2_ID: NodeId = NodeId(unsafe { NonZeroU128::new_unchecked(3) });
const INITIAL_FOCUS: NodeId = CHECKBOX_1_ID;

const CHECKBOX_1_NAME: &str = "Checkbox 1";
const CHECKBOX_2_NAME: &str = "Checkbox 2";

const CHECKBOX_1_RECT: Rect = Rect {
    x0: 20.0,
    y0: 20.0,
    x1: 100.0,
    y1: 60.0,
};

const CHECKBOX_2_RECT: Rect = Rect {
    x0: 20.0,
    y0: 60.0,
    x1: 100.0,
    y1: 100.0,
};

fn build_checkbox(id: NodeId, checked: bool, classes: &mut NodeClassSet) -> Node {
    let (name, rect) = match id {
        CHECKBOX_1_ID => (CHECKBOX_1_NAME, CHECKBOX_1_RECT),
        CHECKBOX_2_ID => (CHECKBOX_2_NAME, CHECKBOX_2_RECT),
        _ => unreachable!(),
    };

    let mut builder = NodeBuilder::new(Role::CheckBox);
    builder.set_bounds(rect);
    builder.set_name(name);
    builder.add_action(Action::Focus);
    builder.set_default_action_verb(DefaultActionVerb::Click);
    builder.set_checked_state(if checked {
        CheckedState::True
    } else {
        CheckedState::False
    });
    builder.build(classes)
}

struct HelloState {
    size: Size,
    focus: NodeId,
    is_window_focused: bool,
    checkbox_1_checked: bool,
    checkbox_2_checked: bool,
    handle: WindowHandle,
    node_classes: NodeClassSet,
}

impl HelloState {
    fn new() -> Self {
        Self {
            size: Default::default(),
            focus: INITIAL_FOCUS,
            is_window_focused: false,
            checkbox_1_checked: false,
            checkbox_2_checked: false,
            handle: Default::default(),
            node_classes: NodeClassSet::new(),
        }
    }

    fn accesskit_focus(&self) -> Option<NodeId> {
        self.is_window_focused.then_some(self.focus)
    }

    fn update_accesskit_focus(&self) {
        self.handle.update_accesskit_if_active(|| TreeUpdate {
            nodes: vec![],
            tree: None,
            focus: self.accesskit_focus(),
        });
    }

    fn toggle_checkbox(&mut self, id: NodeId) {
        let checked = match id {
            CHECKBOX_1_ID => {
                self.checkbox_1_checked = !self.checkbox_1_checked;
                self.checkbox_1_checked
            }
            CHECKBOX_2_ID => {
                self.checkbox_2_checked = !self.checkbox_2_checked;
                self.checkbox_2_checked
            }
            _ => unreachable!(),
        };
        // We have to be slightly less lazy here than we'd like because we can't
        // borrow self immutably inside the closure while we have a mutable
        // borrow of self.node_classes. TBD: Does this indicate a design flaw?
        let focus = self.accesskit_focus();
        let node_classes = &mut self.node_classes;
        self.handle.update_accesskit_if_active(|| {
            let node = build_checkbox(id, checked, node_classes);
            TreeUpdate {
                nodes: vec![(id, node)],
                tree: None,
                focus,
            }
        });
    }
}

impl WinHandler for HelloState {
    fn connect(&mut self, handle: &WindowHandle) {
        self.handle = handle.clone();
    }

    fn prepare_paint(&mut self) {}

    fn paint(&mut self, _: &Region) {}

    fn accesskit_tree(&mut self) -> TreeUpdate {
        let root = {
            let mut builder = NodeBuilder::new(Role::Window);
            builder.set_children([CHECKBOX_1_ID, CHECKBOX_2_ID]);
            builder.set_name(WINDOW_TITLE);
            builder.build(&mut self.node_classes)
        };
        let checkbox_1 = build_checkbox(
            CHECKBOX_1_ID,
            self.checkbox_1_checked,
            &mut self.node_classes,
        );
        let checkbox_2 = build_checkbox(
            CHECKBOX_2_ID,
            self.checkbox_2_checked,
            &mut self.node_classes,
        );
        TreeUpdate {
            nodes: vec![
                (WINDOW_ID, root),
                (CHECKBOX_1_ID, checkbox_1),
                (CHECKBOX_2_ID, checkbox_2),
            ],
            tree: Some(Tree::new(WINDOW_ID)),
            focus: self.accesskit_focus(),
        }
    }

    fn key_down(&mut self, event: KeyEvent) -> bool {
        if event.key == KbKey::Tab {
            self.focus = if self.focus == CHECKBOX_1_ID {
                CHECKBOX_2_ID
            } else {
                CHECKBOX_1_ID
            };
            self.update_accesskit_focus();
            return true;
        }
        if event.key == KbKey::Enter || event.key == KbKey::Character(" ".into()) {
            self.toggle_checkbox(self.focus);
            return true;
        }
        false
    }

    fn size(&mut self, size: Size) {
        self.size = size;
    }

    fn got_focus(&mut self) {
        self.is_window_focused = true;
        self.update_accesskit_focus();
    }

    fn lost_focus(&mut self) {
        self.is_window_focused = false;
        self.update_accesskit_focus();
    }

    fn accesskit_action(&mut self, request: ActionRequest) {
        if let ActionRequest {
            action,
            target,
            data: None,
        } = request
        {
            if target == CHECKBOX_1_ID || target == CHECKBOX_2_ID {
                match action {
                    Action::Focus => {
                        self.focus = target;
                        self.update_accesskit_focus();
                    }
                    Action::Default => {
                        self.toggle_checkbox(target);
                    }
                    _ => (),
                }
            }
        }
    }

    fn request_close(&mut self) {
        self.handle.close();
    }

    fn destroy(&mut self) {
        Application::global().quit()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

fn main() {
    tracing_subscriber::fmt().init();

    let app = Application::new().unwrap();
    let mut builder = WindowBuilder::new(app.clone());
    builder.set_handler(Box::new(HelloState::new()));
    builder.set_title(WINDOW_TITLE);

    let window = builder.build().unwrap();
    window.show();

    app.run(None);
}
