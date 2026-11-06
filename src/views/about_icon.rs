//! macOS標準のAboutパネルに、Dockと同じアプリのアイコンを出す。
//!
//! Aboutパネルは`NSApplicationIcon`という名前の画像を出す。.appに包まない実行ファイルでは、
//! その名前がフォルダの絵に付いている。eframeが登録したアイコンへ名前を付け替える。
use objc2::AnyThread as _;
use objc2_app_kit::{NSApplication, NSImage, NSImageView};
use objc2_foundation::{MainThreadMarker, NSData, ns_string};

/// Dockの表示とAboutの画像を、egui版でeframeが登録していた画像にする。
/// ⌘Tabのアイコンまで替えるsetApplicationIconImageはobjc2でunsafeなので使わず、Dockの表示だけを差し替える。
/// PNGから作るとmacOSによっては合わないlibpngを読み込んで落ちるため（eframeの`app_icon.rs`）、TIFFを受け取る。
/// 主スレッド以外と読めない画像では何もせず`false`を返す。
pub fn set_dock_icon(tiff: &[u8]) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(tiff)) else {
        return false;
    };
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    tile.setContentView(Some(&NSImageView::imageViewWithImage(&image, mtm)));
    tile.display();
    name_for_about(&image)
}

/// アプリのアイコンに`NSApplicationIcon`の名前を付ける。付けられたら`true`。
/// 主スレッド以外、アイコン未登録、名前の付け替えの失敗では何もせず`false`を返す。Aboutがフォルダの絵に戻るだけなので。
pub fn name_app_icon_for_about() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(icon) = NSApplication::sharedApplication(mtm).applicationIconImage() else {
        return false;
    };
    name_for_about(&icon)
}

/// 画像に`NSApplicationIcon`の名前を付ける。名前は1つの画像にしか付かないため、先に今の画像から外す。
fn name_for_about(icon: &NSImage) -> bool {
    let name = ns_string!("NSApplicationIcon");
    if let Some(current) = NSImage::imageNamed(name) {
        current.setName(None);
    }
    icon.setName(Some(name))
}

#[cfg(test)]
mod tests {
    #[test]
    fn does_nothing_off_the_main_thread() {
        assert!(!super::name_app_icon_for_about());
    }
}
