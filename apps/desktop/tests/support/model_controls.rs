use crate::ui;
pub use slint::private_unstable_api::re_exports::AccessibleRole;
use slint::{
    ComponentHandle,
    platform::{PointerEventButton, WindowEvent},
    private_unstable_api::re_exports::{
        AccessibleStringProperty, ItemRc, LogicalRect, WindowInner, ensure_item_tree_instantiated,
    },
};

// Query the same roles and names exposed to assistive technology. Restrict the
// Slint item-tree dependency to this test helper; send real pointer events
// through the public window API so clipped or covered controls cannot pass.
pub fn visible_bounds(
    window: &ui::AppWindow,
    role: AccessibleRole,
    name: &str,
) -> Option<LogicalRect> {
    visible_bounds_matching(window, role, name, |_| true)
}

pub fn visible_bounds_matching(
    window: &ui::AppWindow,
    role: AccessibleRole,
    name: &str,
    matches: impl Fn(&LogicalRect) -> bool,
) -> Option<LogicalRect> {
    let tree = WindowInner::from_pub(window.window()).component();
    ensure_item_tree_instantiated(&tree);
    let mut pending = vec![ItemRc::new_root(tree)];
    while let Some(item) = pending.pop() {
        if item.is_accessible()
            && item.accessible_role() == role
            && item
                .accessible_string_property(AccessibleStringProperty::Label)
                .as_deref()
                == Some(name)
            && item.is_visible()
        {
            let local = item.geometry();
            let rect = LogicalRect::new(item.map_to_window(local.origin), local.size);
            let viewport = window
                .window()
                .size()
                .to_logical(window.window().scale_factor());
            assert!(
                !rect.is_empty()
                    && rect.min_y() >= 0.0
                    && rect.max_y() <= viewport.height
                    && rect.min_x() >= 0.0
                    && rect.max_x() <= viewport.width,
                "{name} must fit in the window: {rect:?}, {viewport:?}"
            );
            if matches(&rect) {
                return Some(rect);
            }
        }
        pending.extend(std::iter::successors(
            item.first_child(),
            ItemRc::next_sibling,
        ));
    }
    None
}

pub fn bounds(window: &ui::AppWindow, role: AccessibleRole, name: &str) -> LogicalRect {
    visible_bounds(window, role, name).unwrap_or_else(|| {
        eprintln!("Could not find visible {role:?} named {name:?}");
        std::process::exit(1)
    })
}

pub fn click(window: &ui::AppWindow, role: AccessibleRole, name: &str) {
    let point = bounds(window, role, name).center();
    click_at(window, point.x, point.y);
}

pub fn click_at(ui: &ui::AppWindow, x: f32, y: f32) {
    for event in [
        WindowEvent::PointerPressed {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        },
        WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        },
    ] {
        assert!(ui.window().try_dispatch_event(event).is_ok());
    }
}
