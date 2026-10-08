//! macOS標準のAboutパネルに、Dockと同じアプリのアイコンを出す。
//!
//! Aboutパネルは`NSApplicationIcon`という名前の画像を出す。.appに包まない実行ファイルでは、
//! その名前がフォルダの絵に付いている。eframeが登録したアイコンへ名前を付け替える。
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::{MainThreadMarker, ns_string};

/// アプリのアイコンに`NSApplicationIcon`の名前を付ける。付けられたら`true`。
/// 主スレッド以外、アイコン未登録、名前の付け替えの失敗では何もせず`false`を返す。Aboutがフォルダの絵に戻るだけなので。
pub fn name_app_icon_for_about() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(icon) = NSApplication::sharedApplication(mtm).applicationIconImage() else {
        return false;
    };
    let name = ns_string!("NSApplicationIcon");
    // 名前は1つの画像にしか付かないため、先にシステムの画像から外す。
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
