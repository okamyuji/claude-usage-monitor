//! 標準のAboutパネルは`NSApplicationIcon`という名前の画像を出す。アプリのアイコンにその名前が付くかを、主スレッドで確かめる。

#[cfg(target_os = "macos")]
fn main() {
    use claude_usage_monitor::views::about_icon::name_app_icon_for_about;
    use objc2::AnyThread;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSSize, ns_string};

    let app = NSApplication::sharedApplication(MainThreadMarker::new().expect("主スレッド"));
    let icon = NSImage::initWithSize(NSImage::alloc(), NSSize::new(16.0, 16.0));
    // SAFETY: objc2-app-kitがunsafeにしている理由は`None`を渡せない場合があることだけで、ここは`Some`を渡す。
    unsafe { app.setApplicationIconImage(Some(&icon)) };
    // applicationIconImageは登録した画像のコピーを返すため、同一性ではなく大きさで見分ける。システムの画像は128px。
    let named_size = || NSImage::imageNamed(ns_string!("NSApplicationIcon")).map(|i| i.size());
    assert_ne!(
        named_size(),
        Some(icon.size()),
        "前提: 名前はまだアプリのアイコンを指していない"
    );

    assert!(name_app_icon_for_about());

    assert_eq!(
        named_size(),
        Some(icon.size()),
        "Aboutの画像がアプリのアイコンになる"
    );

    use claude_usage_monitor::views::about_icon::set_dock_icon;
    let tiff = include_bytes!("../assets/app-icon.tiff");
    assert!(set_dock_icon(tiff));
    let dock_image = app
        .dockTile()
        .contentView(MainThreadMarker::new().unwrap())
        .and_then(|v| v.downcast::<objc2_app_kit::NSImageView>().ok())
        .and_then(|v| v.image())
        .map(|i| i.size());
    assert_eq!(
        dock_image,
        Some(NSSize::new(512.0, 512.0)),
        "Dockに512ptの画像を出す"
    );
    assert_eq!(
        named_size(),
        Some(NSSize::new(512.0, 512.0)),
        "Aboutにも512ptの画像を出す"
    );
    assert!(
        !set_dock_icon(b"not an image"),
        "読めない画像では何もしない"
    );
    println!("about_icon: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
