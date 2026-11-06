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

/// 入力欄が書き換える`Forms`の欄。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// 履歴の検索。
    SessionQuery,
    /// 追加するプロファイルの名前。
    ProfileName,
    /// 追加するプロファイルの設定ディレクトリ。
    ProfileDir,
    /// 取得間隔（秒）。
    Interval,
    /// 通知閾値（%）。
    Threshold,
    /// 保持日数。
    Retention,
    /// ヘッドレスの稼働判定（秒）。
    Headless,
    /// ジョブの稼働判定（分）。
    Job,
    /// 編集中のモデルの入力の単価。
    ModelInput,
    /// 編集中のモデルの出力の単価。
    ModelOutput,
    /// 編集中のモデルのキャッシュ読込の単価。
    ModelCacheRead,
    /// 編集中のモデルのキャッシュ作成5分の単価。
    ModelCacheWrite5m,
    /// 編集中のモデルのキャッシュ作成1時間の単価。
    ModelCacheWrite1h,
    /// 編集中のモデルのコンテキスト長。
    ModelContext,
}

impl Field {
    /// 描く順。`FormInputs::values`もこの順に返す。
    pub const ALL: [Field; 14] = [
        Field::SessionQuery,
        Field::ProfileName,
        Field::ProfileDir,
        Field::Interval,
        Field::Threshold,
        Field::Retention,
        Field::Headless,
        Field::Job,
        Field::ModelInput,
        Field::ModelOutput,
        Field::ModelCacheRead,
        Field::ModelCacheWrite5m,
        Field::ModelCacheWrite1h,
        Field::ModelContext,
    ];

    /// 欄の値。編集していない単価の欄は空。
    fn get(self, f: &Forms) -> &str {
        let s = &f.settings;
        let m = f.model.as_ref();
        let model = |pick: fn(&crate::controllers::gui::tabs::settings::ModelDraft) -> &String| {
            m.map_or("", |m| pick(m).as_str())
        };
        match self {
            Field::SessionQuery => &f.session_query,
            Field::ProfileName => &f.new_profile_name,
            Field::ProfileDir => &f.new_profile_dir,
            Field::Interval => &s.interval,
            Field::Threshold => &s.threshold,
            Field::Retention => &s.retention,
            Field::Headless => &s.headless,
            Field::Job => &s.job,
            Field::ModelInput => model(|m| &m.input),
            Field::ModelOutput => model(|m| &m.output),
            Field::ModelCacheRead => model(|m| &m.cache_read),
            Field::ModelCacheWrite5m => model(|m| &m.cache_write_5m),
            Field::ModelCacheWrite1h => model(|m| &m.cache_write_1h),
            Field::ModelContext => model(|m| &m.context),
        }
    }

    /// 欄の場所。単価の欄は編集中（`Forms::model`がある）だけある。
    fn get_mut(self, f: &mut Forms) -> Option<&mut String> {
        let s = &mut f.settings;
        let m = f.model.as_mut();
        Some(match self {
            Field::SessionQuery => &mut f.session_query,
            Field::ProfileName => &mut f.new_profile_name,
            Field::ProfileDir => &mut f.new_profile_dir,
            Field::Interval => &mut s.interval,
            Field::Threshold => &mut s.threshold,
            Field::Retention => &mut s.retention,
            Field::Headless => &mut s.headless,
            Field::Job => &mut s.job,
            Field::ModelInput => &mut m?.input,
            Field::ModelOutput => &mut m?.output,
            Field::ModelCacheRead => &mut m?.cache_read,
            Field::ModelCacheWrite5m => &mut m?.cache_write_5m,
            Field::ModelCacheWrite1h => &mut m?.cache_write_1h,
            Field::ModelContext => &mut m?.context,
        })
    }

    /// 打つたびにcontrollerへ送る操作。検索は打つたびに絞り込む。
    fn on_change(self) -> Option<Action> {
        (self == Field::SessionQuery).then_some(Action::Dash(DashAction::Search))
    }

    /// 空のときに淡く出す例（eguiの`hint_text`）。
    fn placeholder(self) -> Option<&'static str> {
        (self == Field::ProfileDir).then_some("空なら ~/.claude-<名前>")
    }
}

struct Slot {
    state: Entity<InputState>,
    /// 前に見た`Forms`の値。
    seen: String,
}

/// 画面にある文字の入力欄。
pub struct FormInputs {
    slots: Vec<Slot>,
    _subs: Vec<Subscription>,
}

impl FormInputs {
    /// 入力欄を作り、変更を`Forms`へ書き戻す。
    pub fn new(window: &mut Window, cx: &mut Context<CumonRoot>) -> Self {
        let mut slots = Vec::with_capacity(Field::ALL.len());
        let mut subs = Vec::with_capacity(Field::ALL.len());
        for (i, field) in Field::ALL.into_iter().enumerate() {
            let state = cx.new(|cx| {
                let s = InputState::new(window, cx);
                match field.placeholder() {
                    Some(h) => s.placeholder(h),
                    None => s,
                }
            });
            subs.push(
                cx.subscribe_in(&state, window, move |root, state, ev, _, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    let v = state.read(cx).value().to_string();
                    root.inputs.slots[i].seen.clone_from(&v);
                    // 単価の欄は編集を閉じた後にも通知が届くことがあるので、欄が無ければ捨てる。
                    if let Some(place) = field.get_mut(root.controller_mut().forms_mut()) {
                        *place = v;
                    }
                    if let Some(a) = field.on_change() {
                        root.act(a, cx);
                    }
                }),
            );
            slots.push(Slot {
                state,
                seen: String::new(),
            });
        }
        Self { slots, _subs: subs }
    }

    /// 欄の入力欄。
    pub fn get(&self, field: Field) -> &Entity<InputState> {
        &self.slots[Self::index(field)].state
    }

    fn index(field: Field) -> usize {
        Field::ALL
            .iter()
            .position(|f| *f == field)
            .expect("Field::ALLにすべての欄がある")
    }

    /// controllerが書き換えた値を入力欄へ戻す。
    pub fn sync(&mut self, forms: &Forms, window: &mut Window, cx: &mut App) {
        for (field, slot) in Field::ALL.into_iter().zip(&mut self.slots) {
            let want = field.get(forms);
            if want == slot.seen {
                continue;
            }
            slot.seen = want.to_string();
            let want = want.to_string();
            slot.state.update(cx, |s, cx| s.set_value(want, window, cx));
        }
    }

    /// 入力欄の値を、`Field::ALL`の順に返す。
    pub fn values(&self, cx: &App) -> Vec<String> {
        self.slots
            .iter()
            .map(|s| s.state.read(cx).value().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::gui::tabs::settings::ModelDraft;

    fn draft() -> ModelDraft {
        ModelDraft {
            prefix: "claude-opus-5-5".into(),
            name: "Opus 5.5".into(),
            input: String::new(),
            output: String::new(),
            cache_read: String::new(),
            cache_write_5m: String::new(),
            cache_write_1h: String::new(),
            context: String::new(),
        }
    }

    #[test]
    fn every_field_reads_back_what_was_written() {
        let mut f = Forms {
            model: Some(draft()),
            ..Forms::default()
        };
        for (i, field) in Field::ALL.into_iter().enumerate() {
            *field.get_mut(&mut f).expect("編集中は単価の欄もある") = format!("v{i}");
        }
        for (i, field) in Field::ALL.into_iter().enumerate() {
            assert_eq!(field.get(&f), format!("v{i}"), "{field:?}");
        }
    }

    #[test]
    fn model_fields_are_empty_and_absent_without_a_draft() {
        let mut f = Forms::default();
        for field in [Field::ModelInput, Field::ModelContext] {
            assert_eq!(field.get(&f), "");
            assert!(field.get_mut(&mut f).is_none());
        }
        assert!(Field::Threshold.get_mut(&mut f).is_some());
    }

    #[test]
    fn only_search_acts_on_change_and_only_dir_has_a_placeholder() {
        for field in Field::ALL {
            assert_eq!(field.on_change().is_some(), field == Field::SessionQuery);
            assert_eq!(field.placeholder().is_some(), field == Field::ProfileDir);
        }
    }
}
