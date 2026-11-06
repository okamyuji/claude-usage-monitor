//! 推移グラフ（spec 7.2節の下段）。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::trend::{TrendPanel, TrendRange, TrendsVm};
use crate::controllers::gui::dashboard::{DashAction, DashboardVm};
use crate::gpui_views::app::{Acts, Plots};
use crate::gpui_views::icons;
use crate::gpui_views::plot::format::{hover_label, plot_time};
use crate::gpui_views::plot::{Plot, PlotState};
use crate::gpui_views::theme::{GAP_X, Palette, SMALL};
use crate::gpui_views::widgets::{help, icon, section, selectable, small_button, text};
use crate::views::layout::GAP;
use chrono::FixedOffset;
use gpui::{Entity, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};

/// 見出しの行の高さ（eguiの`interact_size.y`）。グラフの高さはここから引く。
const ROW: f32 = 18.0;

/// 推移の区画を描く。見出しの行は折りたたみ中も出す。`height`は区画に使える高さ。
pub fn render(
    vm: &DashboardVm,
    p: &'static Palette,
    acts: &Acts,
    plots: &mut Plots,
    height: f32,
) -> impl IntoElement {
    let (caret, toggle) = if vm.trend_open {
        (icons::CARET_DOWN, "折りたたむ")
    } else {
        (icons::CARET_RIGHT, "開く")
    };
    let current = vm.trend.as_ref().map(|t| t.range);
    let ranges = [TrendRange::Hours5, TrendRange::Hours24, TrendRange::Days7].map(|r| {
        selectable(
            p,
            r.label(),
            current == Some(r),
            r.label(),
            acts.on(Action::Dash(DashAction::SetTrendRange(r))),
        )
    });
    let head = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(icon(caret, p.text))
        .child(section(p, "推移"))
        .children(ranges)
        .child(small_button(
            p,
            "trend-toggle",
            toggle,
            acts.on(Action::Dash(DashAction::ToggleTrend)),
        ));
    let body = vm.trend.as_ref().map(|t| {
        let mut panels = Vec::with_capacity(t.panels.len());
        for pn in &t.panels {
            let state = plots.get(&title(pn));
            panels.push(panel(t, pn, p, state, height - ROW));
        }
        div().flex().gap(px(GAP)).children(panels)
    });
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(head)
        .children(body)
}

/// グラフのⓘの説明。目印の線は凡例に出すと低いグラフで凡例がはみ出すため、名前を空にして凡例から外し、ここで意味を伝える。
fn graph_help(base: &str) -> String {
    format!("{base}。縦の灰色の線はリセット時刻、横の線は通知閾値です")
}

fn title(pn: &TrendPanel) -> String {
    format!("{}のグラフ", pn.title)
}

fn panel(
    t: &TrendsVm,
    pn: &TrendPanel,
    p: &'static Palette,
    state: Entity<PlotState>,
    height: f32,
) -> impl IntoElement {
    let tz =
        FixedOffset::east_opt(t.tz_offset_secs).unwrap_or(FixedOffset::east_opt(0).expect("UTC"));
    let title = title(pn);
    let mut plot = Plot::new(SharedString::from(title.clone()), state, p)
        .height(px((height - ROW).max(60.0)))
        // 凡例はプロファイルごとに1行増える。本文の大きさでは、既定のウィンドウの高さで数行でもグラフの下端からはみ出す。
        .legend(true)
        .include_y(0.0)
        .include_y(100.0)
        .include_x(t.x_min)
        .include_x(t.x_max)
        .x_formatter(move |x| plot_time(x, tz))
        .hover_formatter(move |name, x, y| hover_label(name, x, y, tz, "%"));
    for s in &pn.series {
        plot = plot.line(s.name.clone(), s.points.clone());
    }
    for r in &pn.resets {
        plot = plot.vline(*r, p.weak);
    }
    plot = plot.hline(t.threshold, p.warn);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_X)
                .child(text(&title, SMALL, p.weak))
                .child(help(p, &graph_help(pn.help))),
        )
        .child(plot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::gui::app::{Action, Tab, TabVm};
    use crate::gpui_views::app::tests::{click, controller, open};
    use crate::gpui_views::probe::{self, Role};

    #[test]
    fn graph_help_explains_marker_lines() {
        let s = graph_help("直近5時間の割合です");
        assert!(s.starts_with("直近5時間の割合です。"));
        assert!(s.contains("縦の灰色の線はリセット時刻"));
        assert!(s.contains("横の線は通知閾値"));
    }

    #[test]
    fn trend_shows_ranges_panels_and_collapses() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut cx = crate::test_support::gpui_cx();
        let (root, vcx) = open(&mut cx, ctl);
        root.update(vcx, |r, cx| r.act(Action::SelectTab(Tab::Dashboard), cx));
        vcx.run_until_parked();
        let seen = probe::take();
        for (role, label) in [
            (Role::Label, "推移"),
            (Role::Tab, "5時間"),
            (Role::Tab, "24時間"),
            (Role::Tab, "7日"),
            (Role::Button, "折りたたむ"),
        ] {
            assert!(seen.contains(&(role, label.into())), "{label}: {seen:?}");
        }
        assert!(seen.iter().any(|(_, l)| l.ends_with("のグラフ")));
        click(vcx, Role::Tab, "24時間");
        let range = root.read_with(vcx, |r, _| match &r.controller().vm().body {
            TabVm::Dashboard(d) => d.trend.as_ref().map(|t| t.range),
            _ => None,
        });
        assert_eq!(
            range,
            Some(crate::controllers::gui::dashboard::trend::TrendRange::Hours24)
        );
        click(vcx, Role::Button, "折りたたむ");
        let seen = probe::take();
        assert!(seen.contains(&(Role::Button, "開く".into())));
        assert!(
            !seen.iter().any(|(_, l)| l.ends_with("のグラフ")),
            "折りたたむとグラフを出さない"
        );
    }
}
