//! メニューバーのトレイ（tray-icon）と、それを動かすwinitのイベントループ。
//!
//! macOSはトレイの操作をメインスレッドに限るため、周期処理を別スレッドへ出し、メインスレッドでイベントループを回す（spec 4.2節）。
use crate::controllers::daemon::tray::{TrayAction, TrayController, TrayOutcome, TrayVm};
use crate::models::ports::Notifier;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// メニューの操作を拾う間隔。DBの読み直しもこの間隔で行うが、読むのは最新の取得結果の数行だけなので負荷は小さい。
const POLL: Duration = Duration::from_millis(250);
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

/// メニューの1項目。tray-iconの型から切り離し、組み立てをテストできるようにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// 通常の項目。
    Item(String, TrayAction),
    /// チェック付きの項目（プロファイル）。
    Check(String, bool, TrayAction),
    /// 区切り線。
    Separator,
}

/// メニューの項目を並べる（spec 7.5節の、画面を開く、プロファイル切替、一時停止、終了）。
pub fn menu_entries(vm: &TrayVm) -> Vec<Entry> {
    let mut e = vec![
        Entry::Item("画面を開く".into(), TrayAction::OpenGui),
        Entry::Separator,
    ];
    e.extend(
        vm.profiles
            .iter()
            .map(|(n, a)| Entry::Check(n.clone(), *a, TrayAction::UseProfile(n.clone()))),
    );
    e.push(Entry::Separator);
    e.push(Entry::Item(
        if vm.paused { "再開" } else { "一時停止" }.into(),
        TrayAction::TogglePause,
    ));
    e.push(Entry::Item("終了".into(), TrayAction::Quit));
    e
}

/// トレイのアイコン画像。青い丸を描いたRGBAを作る。画像ファイルを同梱しないため。
pub fn tray_icon_rgba(size: u32) -> Vec<u8> {
    let c = size as f32 / 2.0;
    let r2 = (c - 1.0) * (c - 1.0);
    (0..size * size)
        .flat_map(|i| {
            let (x, y) = ((i % size) as f32 + 0.5 - c, (i / size) as f32 + 0.5 - c);
            if x * x + y * y <= r2 {
                [0x3b, 0x82, 0xf6, 255]
            } else {
                [0, 0, 0, 0]
            }
        })
        .collect()
}

fn build_menu(vm: &TrayVm) -> (Menu, HashMap<MenuId, TrayAction>) {
    let menu = Menu::new();
    let mut ids = HashMap::new();
    for e in menu_entries(vm) {
        match e {
            Entry::Item(text, a) => {
                let item = MenuItem::new(text, true, None);
                ids.insert(item.id().clone(), a);
                let _ = menu.append(&item);
            }
            Entry::Check(text, checked, a) => {
                let item = CheckMenuItem::new(text, true, checked, None);
                ids.insert(item.id().clone(), a);
                let _ = menu.append(&item);
            }
            Entry::Separator => {
                let _ = menu.append(&PredefinedMenuItem::separator());
            }
        }
    }
    (menu, ids)
}

/// 周期処理のスレッドが終わったことを知らせるイベント。
#[derive(Debug)]
enum UserEvent {
    WorkerDone,
}

struct App {
    ctl: TrayController,
    notifier: Arc<dyn Notifier>,
    tray: Option<TrayIcon>,
    ids: HashMap<MenuId, TrayAction>,
    shown: Option<TrayVm>,
}

impl App {
    /// 表示内容が変わったときだけメニューを作り直す。毎回作り直すとメニューを開いている間に閉じてしまうため。
    fn refresh(&mut self) {
        let Ok(vm) = self.ctl.vm() else {
            return;
        };
        if self.shown.as_ref() == Some(&vm) {
            return;
        }
        let (menu, ids) = build_menu(&vm);
        if let Some(t) = &self.tray {
            t.set_menu(Some(Box::new(menu)));
            t.set_title(Some(vm.title.clone()));
            let _ = t.set_tooltip(Some(vm.tooltip.clone()));
        }
        self.ids = ids;
        self.shown = Some(vm);
    }

    fn create_tray(&mut self) -> Result<(), String> {
        let vm = self.ctl.vm().map_err(|e| e.to_string())?;
        let (menu, ids) = build_menu(&vm);
        let icon = Icon::from_rgba(tray_icon_rgba(32), 32, 32).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_title(&vm.title)
            .with_tooltip(&vm.tooltip)
            .build()
            .map_err(|e| format!("トレイを作れません: {e}"))?;
        self.tray = Some(tray);
        self.ids = ids;
        self.shown = Some(vm);
        Ok(())
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if cause == StartCause::Init
            && let Err(e) = self.create_tray()
        {
            eprintln!("cumon: {e}");
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let Some(a) = self.ids.get(&ev.id).cloned() else {
                continue;
            };
            match self.ctl.handle(a) {
                TrayOutcome::Continue => {}
                TrayOutcome::Quit => {
                    event_loop.exit();
                    return;
                }
                TrayOutcome::Message(m) => {
                    let _ = self.notifier.notify("Claude Usage Monitor", &m);
                }
            }
        }
        self.refresh();
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + POLL));
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::WorkerDone => event_loop.exit(),
        }
    }
}

fn event_loop() -> Result<EventLoop<UserEvent>, String> {
    let mut b = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        b.with_activation_policy(ActivationPolicy::Accessory);
    }
    b.build()
        .map_err(|e| format!("イベントループを作れません: {e}"))
}

/// トレイを出し、`worker`（周期処理）を別スレッドで回す。`worker`が終わるか、トレイで「終了」を選ぶと戻る。
///
/// デスクトップのない環境などでイベントループを作れないときは、トレイなしで`worker`をこのスレッドで回す。
/// 常駐の目的は収集なので、トレイが出せないことを理由に止めないため。
pub fn run_with_tray(
    ctl: TrayController,
    notifier: Arc<dyn Notifier>,
    worker: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    let el = match event_loop() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("cumon: {e}。トレイなしで続けます");
            worker();
            return Ok(());
        }
    };
    let proxy: EventLoopProxy<UserEvent> = el.create_proxy();
    let handle = std::thread::spawn(move || {
        worker();
        let _ = proxy.send_event(UserEvent::WorkerDone);
    });
    let mut app = App {
        ctl,
        notifier,
        tray: None,
        ids: HashMap::new(),
        shown: None,
    };
    el.run_app(&mut app)
        .map_err(|e| format!("イベントループが止まりました: {e}"))?;
    handle
        .join()
        .map_err(|_| "周期処理のスレッドが異常終了しました".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_square_rgba_with_transparent_corners() {
        let px = tray_icon_rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        assert_eq!(&px[0..4], &[0, 0, 0, 0]);
        let center = ((16 * 32 + 16) * 4) as usize;
        assert_eq!(px[center + 3], 255);
    }

    #[test]
    fn menu_entries_follow_vm() {
        let vm = TrayVm {
            title: "13%・67%".into(),
            tooltip: String::new(),
            profiles: vec![("default".into(), true), ("sub".into(), false)],
            paused: true,
        };
        let e = menu_entries(&vm);
        assert_eq!(
            e,
            [
                Entry::Item("画面を開く".into(), TrayAction::OpenGui),
                Entry::Separator,
                Entry::Check(
                    "default".into(),
                    true,
                    TrayAction::UseProfile("default".into())
                ),
                Entry::Check("sub".into(), false, TrayAction::UseProfile("sub".into())),
                Entry::Separator,
                Entry::Item("再開".into(), TrayAction::TogglePause),
                Entry::Item("終了".into(), TrayAction::Quit),
            ]
        );
    }
}
