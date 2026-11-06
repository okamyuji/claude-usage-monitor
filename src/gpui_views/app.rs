//! 画面の枠（上部バー、本文、状態表示）とGPUIのアプリ。
use crate::controllers::gui::app::{Action, AppVm, GuiController, REFRESH_SECS};
use crate::gpui_views::fonts::{BODY_FAMILY, body_fallbacks, install};
use crate::gpui_views::header;
use crate::gpui_views::theme::{GAP_X, Palette, color, palette};
use crate::gpui_views::widgets::{self, frame_start, label, text};
use crate::models::domain::settings::Theme;
use gpui::{
    App, AppContext as _, Bounds, ClickEvent, Context, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowAppearance,
    WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};
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

/// 根のビュー。controllerを持ち、描き直しと操作の受け口になる。
pub struct CumonRoot {
    ctl: GuiController,
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
        Self {
            ctl,
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
        let root = self.0.clone();
        move |_, _, cx| {
            // 窓を閉じた後に届いた操作は、受け取る画面が無いので捨てる。
            let _ = root.update(cx, |r, cx| r.act(a.clone(), cx));
        }
    }
}

impl Render for CumonRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        frame_start();
        let vm = self.ctl.vm();
        let dark = is_dark(vm.theme, window.appearance());
        let p = palette(dark);
        let acts = Acts(cx.entity().downgrade());
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(color(p.bg))
            .text_color(color(p.text))
            .font_family(BODY_FAMILY);
        root.text_style().font_fallbacks = Some(body_fallbacks());
        root.child(
            div()
                .bg(color(p.card))
                .border_b_1()
                .border_color(color(p.border))
                .px(px(12.0))
                .py(px(8.0))
                .child(header::render(vm, p, dark, &acts)),
        )
        .child(div().flex_1().min_h_0().p(px(12.0)).child(body(vm, p)))
        .child(status(vm, p))
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

// shortcut: タブの中身は移す途中のためタブ名だけを出す。各タブを移したらその画面を描く。
fn body(vm: &AppVm, p: &Palette) -> impl IntoElement {
    label(p, vm.tab.label())
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
        let key: &'static str = Box::leak(probe::selector(role, label).into_boxed_str());
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
