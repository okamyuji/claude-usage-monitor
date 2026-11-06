//! 診断タブの描画。
use crate::controllers::gui::tabs::diagnostics::DiagnosticsVm;
use crate::gpui_views::app::Plots;
use crate::gpui_views::plot::Plot;
use crate::gpui_views::plot::format::{hover_label, plot_time};
use crate::gpui_views::theme::{BODY, GAP_X, Palette, SMALL};
use crate::gpui_views::widgets::{card, help, label, section, text, trunc, truncate};
use crate::models::domain::display::help as h;
use crate::views::layout::GAP;
use chrono::FixedOffset;
use gpui::{IntoElement, ParentElement as _, Styled as _, div, px};

/// 列（対象、時刻、結果）の幅。詳細の列だけが伸びる。
const COLUMNS: [f32; 3] = [180.0, 72.0, 56.0];
/// 伸びる列の最小の幅（`views::layout`の`MIN_FLEX`と同じ）。
const MIN_FLEX: f32 = 40.0;

/// 診断タブを描く。
pub fn render(vm: &DiagnosticsVm, p: &'static Palette, plots: &mut Plots) -> impl IntoElement {
    let rows = vm.fetches.iter().map(|f| {
        div()
            .flex()
            .items_center()
            .gap(px(GAP / 2.0))
            .child(
                div()
                    .w(px(COLUMNS[0]))
                    .flex_none()
                    .overflow_hidden()
                    .child(trunc(p, &f.target)),
            )
            .child(
                div()
                    .w(px(COLUMNS[1]))
                    .flex_none()
                    .child(text(&f.at, SMALL, p.text)),
            )
            .child(div().w(px(COLUMNS[2])).flex_none().child(text(
                &f.result,
                BODY,
                if f.failed { p.err } else { p.ok },
            )))
            .child(
                div()
                    .flex_1()
                    .min_w(px(MIN_FLEX))
                    .overflow_hidden()
                    .child(truncate(p, text(&f.detail, SMALL, p.text), &f.detail)),
            )
    });
    let tz =
        FixedOffset::east_opt(vm.tz_offset_secs).unwrap_or(FixedOffset::east_opt(0).expect("UTC"));
    let rss = Plot::new("rss", plots.get("rss"), p)
        .height(px(180.0))
        .include_y(0.0)
        .x_formatter(move |x| plot_time(x, tz))
        .hover_formatter(move |name, x, y| hover_label(name, x, y, tz, "MB"))
        .line("RSS（MB）", vm.rss_points.clone());
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(
            card(p, false)
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(section(p, "取得の状況"))
                .children(rows)
                .child(text(&vm.ingest, SMALL, p.weak)),
        )
        .child(
            card(p, false)
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(GAP_X)
                        .child(section(
                            p,
                            &format!("デーモンのメモリ（最新 {}）", vm.rss_latest),
                        ))
                        .child(help(p, h::RSS)),
                )
                .child(rss)
                .child(label(p, &format!("DBサイズ {}", vm.db_size))),
        )
}

#[cfg(test)]
mod tests {
    use crate::controllers::gui::app::{Action, Tab};
    use crate::gpui_views::app::tests::{controller, open};
    use crate::gpui_views::probe::{self, Role};

    #[test]
    fn diagnostics_shows_fetches_memory_graph_and_db_size() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut cx = crate::test_support::gpui_cx();
        let (root, vcx) = open(&mut cx, ctl);
        root.update(vcx, |r, cx| r.act(Action::SelectTab(Tab::Diagnostics), cx));
        vcx.run_until_parked();
        let seen = probe::take();
        assert!(seen.contains(&(Role::Label, "取得の状況".into())));
        assert!(
            seen.iter()
                .any(|(_, l)| l.starts_with("デーモンのメモリ（最新 "))
        );
        assert!(seen.iter().any(|(_, l)| l.starts_with("DBサイズ ")));
        assert!(seen.contains(&(Role::Label, "ⓘ".into())));
    }
}
