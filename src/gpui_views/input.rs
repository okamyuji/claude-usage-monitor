//! 文字の入力欄とcontrollerの`Forms`の同期。
//!
//! 入力欄の値は入力欄のentityが持つ。利用者が打った値は変更の通知で`Forms`へ書き戻し、
//! controllerが`Forms`を書き換えた値（例: 追加の後に空にする）は描画の前に入力欄へ戻す。
//! IMEの変換中の文字は変更の通知を出さず、入力欄にだけある（gpui-baseの`replace_and_mark_text_in_range`）。
//! 入力欄と`Forms`の差では戻さず、`Forms`が前に見た値から変わったときだけ戻して、変換中の文字を消さない。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::dashboard::DashAction;
use crate::gpui_views::app::CumonRoot;
use gpui::{App, AppContext as _, Context, Entity, Subscription, Window};
use gpui_component::input::{InputEvent, InputState};

/// 画面にある文字の入力欄。
pub struct FormInputs {
    /// 履歴の検索。
    pub session_query: Entity<InputState>,
    /// 前に見た`Forms::session_query`。
    seen_query: String,
    _subs: Vec<Subscription>,
}

impl FormInputs {
    /// 入力欄を作り、変更を`Forms`へ書き戻す。
    pub fn new(window: &mut Window, cx: &mut Context<CumonRoot>) -> Self {
        let session_query = cx.new(|cx| InputState::new(window, cx));
        let sub = cx.subscribe_in(&session_query, window, |root, state, ev, _, cx| {
            if matches!(ev, InputEvent::Change) {
                let v = state.read(cx).value().to_string();
                root.inputs.seen_query.clone_from(&v);
                root.controller_mut().forms_mut().session_query = v;
                root.act(Action::Dash(DashAction::Search), cx);
            }
        });
        Self {
            session_query,
            seen_query: String::new(),
            _subs: vec![sub],
        }
    }

    /// controllerが書き換えた値を入力欄へ戻す。
    pub fn sync(&mut self, forms: &Forms, window: &mut Window, cx: &mut App) {
        if forms.session_query == self.seen_query {
            return;
        }
        self.seen_query.clone_from(&forms.session_query);
        let want = forms.session_query.clone();
        self.session_query
            .update(cx, |s, cx| s.set_value(want, window, cx));
    }

    /// 入力欄の値を、描く順に返す。
    pub fn values(&self, cx: &App) -> Vec<String> {
        vec![self.session_query.read(cx).value().to_string()]
    }
}
