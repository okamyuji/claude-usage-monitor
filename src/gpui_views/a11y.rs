//! 読み上げ用のノードに「無効」を書く包み。
//!
//! GPUIには`aria_disabled`が無いので、見た目を淡くして押しても何もしないだけでは、
//! VoiceOverは押せる部品として読む。eguiの`add_enabled(false, ..)`と同じく無効として読ませるため、
//! 要素の読み上げ用のノードを書いた後に`set_disabled`を足す（gpui-baseの`A11yDisabled`と同じやり方）。
use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, InteractiveElement,
    Interactivity, IntoElement, LayoutId, Pixels, Window, accesskit,
};

/// `disabled`が真のとき、読み上げ用のノードを無効にする要素。
pub struct Disabled<E> {
    inner: E,
    disabled: bool,
}

/// 要素の読み上げ用のノードに無効を書く。
pub trait DisabledExt: Sized {
    /// `disabled`が真なら無効として読ませる。
    fn a11y_disabled(self, disabled: bool) -> Disabled<Self>;
}

impl<E: Element> DisabledExt for E {
    fn a11y_disabled(self, disabled: bool) -> Disabled<Self> {
        Disabled {
            inner: self,
            disabled,
        }
    }
}

impl<E: Element> IntoElement for Disabled<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: InteractiveElement> InteractiveElement for Disabled<E> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl<E: Element> Element for Disabled<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector, bounds, layout, window, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx)
    }

    fn a11y_role(&self) -> Option<gpui::Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.inner.write_a11y_info(node);
        if self.disabled {
            node.set_disabled();
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui::A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{StatefulInteractiveElement as _, div};

    fn node_of(e: impl IntoElement) -> accesskit::Node {
        let e = e.into_element();
        let mut node = accesskit::Node::new(e.a11y_role().unwrap_or(gpui::Role::Unknown));
        e.write_a11y_info(&mut node);
        node
    }

    #[test]
    fn disabled_writes_the_flag_and_keeps_role_and_name() {
        let base = || div().id("b").role(gpui::Role::Button).aria_label("次週");
        let off = node_of(base().a11y_disabled(true));
        assert!(off.is_disabled());
        assert_eq!(off.role(), gpui::Role::Button);
        assert_eq!(off.label(), Some("次週"));
        assert!(!node_of(base().a11y_disabled(false)).is_disabled());
    }
}
