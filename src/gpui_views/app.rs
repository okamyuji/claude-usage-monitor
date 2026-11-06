//! 画面の枠（上部バー、本文、状態表示）とGPUIのアプリ。
use crate::controllers::gui::app::{Action, AppVm, Forms, GuiController, REFRESH_SECS, TabVm};
use crate::controllers::gui::dashboard::live_log::LiveAction;
use crate::controllers::gui::dashboard::memory::MemAction;
use crate::gpui_views::dashboard::detail::DetailUi;
use crate::gpui_views::fonts::{BODY_FAMILY, body_fallbacks, install};
use crate::gpui_views::header;
use crate::gpui_views::input::FormInputs;
use crate::gpui_views::plot::PlotState;
use crate::gpui_views::theme::{GAP_X, Palette, color, palette};
use crate::gpui_views::widgets::{self, frame_start, label, text};
use crate::models::domain::settings::Theme;
use crate::views::layout::drag_ratio;
use gpui::{
    App, AppContext as _, Bounds, ClickEvent, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowAppearance, WindowBounds,
    WindowKind, WindowOptions, canvas, div, point, px, size,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// テーマの選択と窓の明暗から、ダークで描くかを決める。
pub fn is_dark(t: Theme, appearance: WindowAppearance) -> bool {
    match t {
        Theme::Light => false,
        Theme::Dark => true,
        Theme::System => matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    }
}

/// 操作が無くても描き直す間隔。5秒ごとの読み直しと経過時間の表示を進めるため。再生中は短くする。
pub fn redraw_every(playing: bool) -> Duration {
    Duration::from_millis(if playing { 50 } else { 1000 })
}

/// グラフごとの状態。グラフの題名で引き、描画をまたいで持つ。
pub struct Plots<'a> {
    map: &'a mut HashMap<String, Entity<PlotState>>,
    cx: &'a mut App,
}

impl Plots<'_> {
    /// `key`のグラフの状態。初めてなら作る。
    pub fn get(&mut self, key: &str) -> Entity<PlotState> {
        if let Some(s) = self.map.get(key) {
            return s.clone();
        }
        let s = self.cx.new(|_| PlotState::default());
        self.map.insert(key.to_string(), s.clone());
        s
    }
}

/// 中段のつまみのドラッグ。押した位置と、押したときの空き幅と左の幅。
#[derive(Debug, Clone, Copy)]
struct SplitDrag {
    start_x: f32,
    avail: f32,
    left: f32,
    place: fn(&mut Forms) -> &mut f32,
}

/// 根のビュー。controllerを持ち、描き直しと操作の受け口になる。
pub struct CumonRoot {
    ctl: GuiController,
    pub(crate) plots: HashMap<String, Entity<PlotState>>,
    /// 本文の大きさ。前の描画で測った値で、配置の計算（`views::layout`）に渡す。
    body: Rc<Cell<(f32, f32)>>,
    split_drag: Option<SplitDrag>,
    focus: FocusHandle,
    pub(crate) inputs: FormInputs,
    pub(crate) detail_ui: DetailUi,
    /// カレンダーの詳細。eguiのスクロールの状態は親のuiごとに別なので、タブごとに持つ。
    calendar_detail_ui: DetailUi,
    session_scroll: ScrollHandle,
    calendar_scroll: ScrollHandle,
    _tick: Task<()>,
    _appearance: Subscription,
}

impl CumonRoot {
    /// controllerを受け取って作り、描き直しのタイマーと窓の明暗の監視を始める。
    pub fn new(ctl: GuiController, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tick = cx.spawn(async move |this, cx| {
            loop {
                let Ok(playing) = this.read_with(cx, |r, _| r.ctl.vm().replay_playing()) else {
                    break;
                };
                cx.background_executor().timer(redraw_every(playing)).await;
                if this
                    .update(cx, |r, cx| {
                        r.ctl.tick();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let appearance = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            ctl,
            plots: HashMap::new(),
            body: Rc::new(Cell::new((0.0, 0.0))),
            split_drag: None,
            focus,
            inputs: FormInputs::new(window, cx),
            detail_ui: DetailUi::default(),
            calendar_detail_ui: DetailUi::default(),
            session_scroll: ScrollHandle::new(),
            calendar_scroll: ScrollHandle::new(),
            _tick: tick,
            _appearance: appearance,
        }
    }

    /// controller。
    pub fn controller(&self) -> &GuiController {
        &self.ctl
    }

    /// 書き換えられるcontroller。
    pub fn controller_mut(&mut self) -> &mut GuiController {
        &mut self.ctl
    }

    /// 再生位置のスライダーの値。
    pub fn replay_slider_value(&self, cx: &App) -> Option<f32> {
        self.active_detail().slider_value(cx)
    }

    fn active_detail(&self) -> &DetailUi {
        match self.ctl.vm().body {
            TabVm::Calendar(_) => &self.calendar_detail_ui,
            _ => &self.detail_ui,
        }
    }

    /// 表示中のタブの詳細の状態。
    pub(crate) fn active_detail_mut(&mut self) -> &mut DetailUi {
        match self.ctl.vm().body {
            TabVm::Calendar(_) => &mut self.calendar_detail_ui,
            _ => &mut self.detail_ui,
        }
    }

    /// 文字の入力欄の値を、描く順に返す。
    pub fn input_values(&self, cx: &App) -> Vec<String> {
        self.inputs.values(cx)
    }

    /// 操作を渡し、新しい値で描き直す。
    pub fn act(&mut self, a: Action, cx: &mut Context<Self>) {
        self.ctl.handle(a);
        self.ctl.tick();
        cx.notify();
    }
}

/// 画面の部品から根のビューへ操作を届ける口。
#[derive(Clone)]
pub struct Acts(WeakEntity<CumonRoot>);

impl Acts {
    /// 押したときに`a`を届ける受け口。
    pub fn on(&self, a: Action) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let acts = self.clone();
        move |_, w, cx| acts.send(a.clone(), w, cx)
    }

    /// `a`を届ける。
    pub fn send(&self, a: Action, _: &mut Window, cx: &mut App) {
        // 窓を閉じた後に届いた操作は、受け取る画面が無いので捨てる。
        let _ = self.0.update(cx, |r, cx| r.act(a, cx));
    }

    /// `Forms`を`f`で書き換えてから`a`を届ける受け口。チェックや選択のように、入力欄の値と操作を一緒に変えるときに使う。
    pub fn edit(
        &self,
        f: impl Fn(&mut Forms) + 'static,
        a: Action,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let root = self.0.clone();
        move |_, _, cx| {
            let _ = root.update(cx, |r, cx| {
                f(r.ctl.forms_mut());
                r.active_detail_mut().source_open = false;
                r.act(a.clone(), cx);
            });
        }
    }

    /// 発生元の候補の一覧を開く、または閉じる。
    pub fn set_source_open(&self, open: bool, _: &mut Window, cx: &mut App) {
        let _ = self.0.update(cx, |r, cx| {
            r.active_detail_mut().source_open = open;
            cx.notify();
        });
    }

    /// 中段のつまみのドラッグを始める。動きと離す操作は根のビューが窓全体で受ける。
    /// `(avail, left, place)`は押したときの空き幅、左の幅、書き換える比率の場所。
    pub fn drag_split(
        &self,
        x: Pixels,
        (avail, left, place): (f32, f32, fn(&mut Forms) -> &mut f32),
        _: &mut Window,
        cx: &mut App,
    ) {
        let _ = self.0.update(cx, |r, _| {
            r.split_drag = Some(SplitDrag {
                start_x: f32::from(x),
                avail,
                left,
                place,
            })
        });
    }
}

impl Render for CumonRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        frame_start();
        let acts = Acts(cx.entity().downgrade());
        let on_move = cx.listener(|r, ev: &MouseMoveEvent, _, cx| {
            let Some(d) = r.split_drag else { return };
            if ev.pressed_button != Some(MouseButton::Left) {
                r.split_drag = None;
                return;
            }
            let dx = f32::from(ev.position.x) - d.start_x;
            *(d.place)(r.ctl.forms_mut()) = drag_ratio(d.avail, d.left, dx);
            cx.notify();
        });
        let on_up = cx.listener(|r, _: &MouseUpEvent, _, _| r.split_drag = None);
        let on_key = cx.listener(|r, ev: &KeyDownEvent, _, cx| {
            // eguiのModalと同じく、Escで確認ダイアログを閉じる。
            let open = matches!(&r.ctl.vm().body, TabVm::Dashboard(d) if d.confirm_exit.is_some());
            if open && ev.keystroke.key == "escape" {
                r.act(Action::Memory(MemAction::Cancel), cx);
            }
        });
        let focus = self.focus.clone();
        self.inputs.sync(self.ctl.forms(), window, cx);
        let detail = match &self.ctl.vm().body {
            TabVm::Dashboard(d) => d.detail.as_ref(),
            TabVm::Calendar(c) => c.detail.as_ref(),
            _ => None,
        };
        let replay = detail
            .and_then(|d| d.replay.as_ref())
            .map(|r| (r.len, r.position));
        let jump = detail
            .and_then(|d| d.live.as_ref())
            .is_some_and(|l| l.jump_to_bottom);
        self.active_detail_mut().prepare(replay, jump, window, cx);
        if jump {
            // eguiは最下端へ送った描画の後にJumpDoneを返す。GPUIでは送る指示を出したここで返す。
            self.ctl.handle(Action::Live(LiveAction::JumpDone));
        }
        let Self {
            ctl,
            plots,
            body: body_size,
            inputs,
            session_scroll,
            calendar_scroll,
            detail_ui,
            calendar_detail_ui,
            ..
        } = self;
        let vm = ctl.vm();
        let forms = ctl.forms();
        let dark = is_dark(vm.theme, window.appearance());
        let p = palette(dark);
        // gpui-componentの部品（入力欄、スクロールバー）は自分のテーマで描くので、明暗をアプリに合わせる。
        if gpui_component::ActiveTheme::theme(&**cx).is_dark() != dark {
            let mode = if dark {
                gpui_component::ThemeMode::Dark
            } else {
                gpui_component::ThemeMode::Light
            };
            gpui_component::Theme::change(mode, Some(window), cx);
        }
        let mut plots = Plots { map: plots, cx };
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(color(p.bg))
            .text_color(color(p.text))
            .font_family(BODY_FAMILY);
        root.text_style().font_fallbacks = Some(body_fallbacks());
        let confirm = match &vm.body {
            TabVm::Dashboard(d) => d
                .confirm_exit
                .as_ref()
                .map(|c| crate::gpui_views::dashboard::memory::confirm(c, p, &acts)),
            _ => None,
        };
        root.relative()
            .track_focus(&focus)
            .on_mouse_move(on_move)
            .on_mouse_up(MouseButton::Left, on_up)
            .on_key_down(on_key)
            .child(
                div()
                    .bg(color(p.card))
                    .border_b_1()
                    .border_color(color(p.border))
                    .px(px(12.0))
                    .py(px(8.0))
                    .child(header::render(vm, p, dark, &acts)),
            )
            .child(body(
                vm,
                forms,
                p,
                &acts,
                &mut plots,
                (
                    inputs,
                    session_scroll,
                    calendar_scroll,
                    detail_ui,
                    calendar_detail_ui,
                ),
                body_size,
            ))
            .child(status(vm, p))
            .children(confirm)
    }
}

fn status(vm: &AppVm, p: &Palette) -> impl IntoElement {
    div()
        .bg(color(p.card))
        .border_t_1()
        .border_color(color(p.border))
        .px(px(12.0))
        .py(px(4.0))
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(widgets::text(
            &format!("最終更新 {}（{}秒ごとに更新）", vm.updated, REFRESH_SECS),
            crate::gpui_views::theme::SMALL,
            p.weak,
        ))
        .children(
            vm.notice
                .as_deref()
                .map(|n| text(n, crate::gpui_views::theme::BODY, p.ok)),
        )
        .children(
            vm.error
                .as_deref()
                .map(|e| text(e, crate::gpui_views::theme::BODY, p.err)),
        )
}

// shortcut: 移していないタブはタブ名だけを出し、ダッシュボードは推移だけを出す。各タブを移したらその画面を描く。
fn body(
    vm: &AppVm,
    forms: &Forms,
    p: &'static Palette,
    acts: &Acts,
    plots: &mut Plots,
    (inputs, scroll, calendar_scroll, detail_ui, calendar_detail_ui): (
        &FormInputs,
        &ScrollHandle,
        &ScrollHandle,
        &DetailUi,
        &DetailUi,
    ),
    size: &Rc<Cell<(f32, f32)>>,
) -> impl IntoElement {
    let measured = size.clone();
    // 本文の大きさを測る。変わったら描き直し、次の描画で配置の計算に使う。
    let measure = canvas(
        move |bounds, window, _| {
            let now = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            if measured.replace(now) != now {
                window.refresh();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();
    div().flex_1().min_h_0().p(px(12.0)).child(
        div()
            .relative()
            .size_full()
            .child(measure)
            .child(match &vm.body {
                TabVm::Dashboard(d) => crate::gpui_views::dashboard::render(
                    d,
                    forms,
                    p,
                    acts,
                    plots,
                    (inputs, scroll, detail_ui),
                    size.get(),
                )
                .into_any_element(),
                TabVm::Calendar(c) => crate::gpui_views::tabs::calendar::render(
                    c,
                    forms,
                    (calendar_scroll, calendar_detail_ui),
                    p,
                    acts,
                    size.get().0,
                )
                .into_any_element(),
                TabVm::Diagnostics(v) => div()
                    .id("body-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .child(crate::gpui_views::tabs::diagnostics::render(v, p, plots))
                    .into_any_element(),
                _ => label(p, vm.tab.label()).into_any_element(),
            }),
    )
}

/// `x,y`を窓の左上の位置として読む。
pub fn parse_origin(s: &str) -> Option<(f32, f32)> {
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// アプリのアイコン。egui版でeframeが登録していた既定の画像と同じもの。
#[cfg(target_os = "macos")]
const APP_ICON: &[u8] = include_bytes!("../../assets/app-icon.tiff");

/// 窓の設定。`verify`があれば、その位置に最前面で浮かべ、フォーカスを取らない。
pub fn window_options(verify: Option<(f32, f32)>, cx: &App) -> WindowOptions {
    let window_size = size(px(1280.0), px(820.0));
    let bounds = match verify {
        Some((x, y)) => Bounds::new(point(px(x), px(y)), window_size),
        None => Bounds::centered(None, window_size, cx),
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Claude Usage Monitor".into()),
            ..Default::default()
        }),
        focus: verify.is_none(),
        kind: if verify.is_some() {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        },
        ..Default::default()
    }
}

/// 部品とフォントを用意して窓を開く。窓を閉じたらアプリを終える。
pub fn launch(ctl: GuiController, verify: Option<(f32, f32)>, cx: &mut App) -> gpui::Result<()> {
    gpui_component::init(cx);
    install(cx)?;
    cx.on_window_closed(|cx, _| cx.quit()).detach();
    cx.open_window(window_options(verify, cx), |w, cx| {
        let root = cx.new(|cx| CumonRoot::new(ctl, w, cx));
        cx.new(|cx| gpui_component::Root::new(root, w, cx))
    })?;
    crate::gpui_views::app_menu::install(cx);
    #[cfg(target_os = "macos")]
    crate::views::about_icon::set_dock_icon(APP_ICON);
    if verify.is_none() {
        cx.activate(true);
    }
    Ok(())
}

/// アプリを起動し、窓を開く。窓を閉じたら終わる。
///
/// `CUMON_GPUI_VERIFY=x,y`のときは、その位置に窓を最前面で浮かべ、フォーカスを取らない。
/// GPUIは隠れた窓を描き直さないため、利用者の操作を奪わずに実機で確かめるには見える所に浮かべるしかない。
// shortcut: 移す途中の実機確認だけで使う。GPUI版だけにするときに消す。
pub fn open(ctl: GuiController) -> gpui::Result<()> {
    let verify = std::env::var("CUMON_GPUI_VERIFY")
        .ok()
        .and_then(|v| parse_origin(&v));
    let failed = std::rc::Rc::new(std::cell::RefCell::new(None));
    let out = failed.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(e) = launch(ctl, verify, cx) {
            *out.borrow_mut() = Some(e);
            cx.quit();
        }
    });
    failed.borrow_mut().take().map_or(Ok(()), Err)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::controllers::gui::app::Tab;
    use crate::gpui_views::icons;
    use crate::gpui_views::probe::{self, Role};
    use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
    use chrono::{TimeZone, Utc};
    use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowAppearance};
    use std::collections::HashMap;
    use std::sync::Arc;

    pub(crate) fn controller(home: &std::path::Path) -> (tempfile::TempDir, GuiController) {
        controller_with(home, FakeDaemon::default())
    }

    fn controller_with(
        home: &std::path::Path,
        daemon: FakeDaemon,
    ) -> (tempfile::TempDir, GuiController) {
        let (d, s) = temp_store();
        let clock = Arc::new(FixedClock::at(
            Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap(),
        ));
        let deps = gui_deps(
            Arc::new(s),
            clock,
            home,
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(daemon),
        );
        (d, GuiController::new(deps))
    }

    /// 根のビューを窓に置き、最初の描画まで進める。
    pub(crate) fn open(
        cx: &mut TestAppContext,
        ctl: GuiController,
    ) -> (gpui::Entity<CumonRoot>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::gpui_views::fonts::install(cx).unwrap();
        });
        let mut inner = None;
        let (_, vcx) = cx.add_window_view(|w, cx| {
            let root = cx.new(|cx| CumonRoot::new(ctl, w, cx));
            inner = Some(root.clone());
            gpui_component::Root::new(root, w, cx)
        });
        vcx.run_until_parked();
        (inner.unwrap(), vcx)
    }

    /// 役割と名前で探した部品の中央を押す。
    pub(crate) fn click(vcx: &mut VisualTestContext, role: Role, label: &str) {
        let key: &'static str = Box::leak(probe::selector(role, label, 0).into_boxed_str());
        let b = vcx
            .debug_bounds(key)
            .unwrap_or_else(|| panic!("{key} が無い"));
        vcx.simulate_click(b.center(), Modifiers::default());
        vcx.run_until_parked();
    }

    fn has(seen: &[(Role, String)], role: Role, label: &str) -> bool {
        seen.iter().any(|(r, l)| *r == role && l == label)
    }

    fn has_containing(seen: &[(Role, String)], part: &str) -> bool {
        seen.iter().any(|(_, l)| l.contains(part))
    }

    #[test]
    fn shell_draws_header_status_and_follows_theme_change() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut cx = crate::test_support::gpui_cx();
        let (root, vcx) = open(&mut cx, ctl);
        let seen = probe::take();
        assert!(has(&seen, Role::Tab, "ダッシュボード"));
        assert!(has_containing(&seen, "デーモン停止中"));
        assert!(has_containing(&seen, "最終更新 12:00（5秒ごとに更新）"));
        click(vcx, Role::Button, &format!("{} ダーク", icons::MOON));
        assert_eq!(
            root.read_with(vcx, |r, _| r.controller().vm().theme),
            Theme::Dark
        );
        assert!(has(
            &probe::take(),
            Role::Button,
            &format!("{} ライト", icons::SUN)
        ));
    }

    #[test]
    fn selecting_a_tab_switches_the_body() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut cx = crate::test_support::gpui_cx();
        let (root, vcx) = open(&mut cx, ctl);
        click(vcx, Role::Tab, "診断");
        assert_eq!(
            root.read_with(vcx, |r, _| r.controller().vm().tab),
            Tab::Diagnostics
        );
    }

    #[test]
    fn error_shows_in_the_status_bar() {
        let home = tempfile::tempdir().unwrap();
        let daemon = FakeDaemon {
            fail_start: true,
            ..FakeDaemon::default()
        };
        let (_d, ctl) = controller_with(home.path(), daemon);
        let mut cx = crate::test_support::gpui_cx();
        let (_, vcx) = open(&mut cx, ctl);
        click(vcx, Role::Button, "デーモンを起動");
        assert!(has(
            &probe::take(),
            Role::Label,
            "デーモンを起動できません: テスト"
        ));
    }

    #[test]
    fn redraw_keeps_text_being_composed_in_the_input() {
        use gpui::EntityInputHandler as _;
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut cx = crate::test_support::gpui_cx();
        let (root, vcx) = open(&mut cx, ctl);
        let input = root.read_with(vcx, |r, _| r.inputs.session_query.clone());
        // IMEが変換中の文字を置く。変換中は変更の通知が出ないので、controllerの値は空のまま。
        input.update_in(vcx, |s, window, cx| {
            s.replace_and_mark_text_in_range(None, "やかん", None, window, cx)
        });
        root.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        assert_eq!(
            input.read_with(vcx, |s, _| s.value().to_string()),
            "やかん",
            "描き直しで変換中の文字を消さない"
        );
    }

    #[test]
    fn component_theme_follows_the_app_theme() {
        use gpui_component::ActiveTheme as _;
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut cx = crate::test_support::gpui_cx();
        let (root, vcx) = open(&mut cx, ctl);
        for (t, dark) in [(Theme::Dark, true), (Theme::Light, false)] {
            root.update(vcx, |r, cx| r.act(Action::SetTheme(t), cx));
            vcx.run_until_parked();
            assert_eq!(vcx.update(|_, cx| cx.theme().is_dark()), dark, "{t:?}");
        }
    }

    #[test]
    fn dark_follows_choice_or_window_appearance() {
        use WindowAppearance::*;
        assert!(is_dark(Theme::Dark, Light));
        assert!(!is_dark(Theme::Light, Dark));
        assert!(is_dark(Theme::System, Dark));
        assert!(is_dark(Theme::System, VibrantDark));
        assert!(!is_dark(Theme::System, Light));
        assert!(!is_dark(Theme::System, VibrantLight));
    }

    #[test]
    fn launch_opens_one_window_with_the_shell() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let cx = crate::test_support::gpui_cx();
        cx.update(|cx| launch(ctl, None, cx)).unwrap();
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
        assert!(probe::take().contains(&(Role::Tab, "ダッシュボード".into())));
    }

    #[test]
    fn verify_window_floats_without_focus_at_the_given_origin() {
        let cx = crate::test_support::gpui_cx();
        cx.update(|cx| {
            let normal = window_options(None, cx);
            assert!(normal.focus);
            assert!(matches!(normal.kind, WindowKind::Normal));
            let float = window_options(Some((1552.0, 40.0)), cx);
            assert!(!float.focus);
            assert!(matches!(float.kind, WindowKind::PopUp));
            let Some(WindowBounds::Windowed(b)) = float.window_bounds else {
                panic!("窓の位置が無い");
            };
            assert_eq!(b.origin, point(px(1552.0), px(40.0)));
            assert_eq!(b.size, size(px(1280.0), px(820.0)));
            let title = float.titlebar.and_then(|t| t.title);
            assert_eq!(title.as_deref(), Some("Claude Usage Monitor"));
        });
    }

    #[test]
    fn verify_origin_reads_x_and_y() {
        assert_eq!(parse_origin("1552,40"), Some((1552.0, 40.0)));
        assert_eq!(parse_origin(" -10 , 5 "), Some((-10.0, 5.0)));
        assert_eq!(parse_origin("1552"), None);
        assert_eq!(parse_origin("a,b"), None);
        assert_eq!(parse_origin("1,2,3"), None);
    }

    #[test]
    fn redraw_interval_is_short_only_while_replaying() {
        assert_eq!(redraw_every(false), std::time::Duration::from_millis(1000));
        assert_eq!(redraw_every(true), std::time::Duration::from_millis(50));
    }
}
