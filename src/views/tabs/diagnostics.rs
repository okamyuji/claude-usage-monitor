//! 診断タブの描画。
use crate::controllers::gui::tabs::diagnostics::DiagnosticsVm;
use crate::models::domain::display::help as h;
use crate::views::app::Plots;
use crate::views::layout::GAP;
use crate::views::plot::Plot;
use crate::views::plot::format::{hover_label, plot_time};
use crate::views::theme::{BODY, GAP_X, Palette, SMALL};
use crate::views::widgets::{card, help, label, section, table_row, text, trunc, truncate};
use chrono::FixedOffset;
use gpui::{IntoElement, ParentElement as _, Styled as _, div, px};

/// 列（対象、時刻、結果、詳細）。詳細の列だけが伸びる。
const COLUMNS: [Option<f32>; 4] = [Some(180.0), Some(72.0), Some(56.0), None];

/// 診断タブを描く。
pub fn render(vm: &DiagnosticsVm, p: &'static Palette, plots: &mut Plots) -> impl IntoElement {
    let rows = vm.fetches.iter().map(|f| {
        table_row(
            &COLUMNS,
            vec![
                trunc(p, &f.target).into_any_element(),
                text(&f.at, SMALL, p.text).into_any_element(),
                text(&f.result, BODY, if f.failed { p.err } else { p.ok }).into_any_element(),
                truncate(p, text(&f.detail, SMALL, p.text), &f.detail).into_any_element(),
            ],
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
    use crate::views::app::tests::{controller, open};
    use crate::views::probe::{self, Role};

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
