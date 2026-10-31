//! The None skin: bare video. Nothing is drawn, while the view's root still takes clicks and keys.

use super::SkinRenderer;
use crate::controls::ControlsView;
use gpui_kit::*;
use player::ui::BarPolicy;

pub(crate) struct None;

impl SkinRenderer for None {
    fn bar_policy(&self) -> BarPolicy {
        // No bar is ever drawn, so the policy changes nothing.
        BarPolicy::default()
    }

    fn draws_view(&self) -> bool {
        false
    }

    fn render(&self, _: &ControlsView, _: &mut Window, _: &mut Context<ControlsView>) -> AnyElement {
        Empty.into_any_element()
    }
}
