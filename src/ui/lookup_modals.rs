use super::UmuBoxApp;
use crate::deps::{DependencyReport, PREFIX_PRESETS, analyze_exe_dependencies, read_installed_winetricks_verbs};
use crate::gamedb::{
    ProtonDbSummary, UmuDbEntry, extract_steam_appid, fetch_protondb_summary,
    find_protonfix_for_game_id, open_url_in_browser, protondb_url, search_umu_database,
};
use crate::theme::{accent_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, PathPromptOptions, StatefulInteractiveElement as _, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

pub fn protondb_tier_color(tier: &str) -> gpui_kit::Hsla {
    match tier.trim().to_ascii_lowercase().as_str() {
        "platinum" => hex_color(kd::AQUA),
        "gold" => hex_color(kd::YELLOW),
        "silver" => hex_color(kd::FG_SUBTLE),
        "bronze" => hex_color(kd::ORANGE),
        "borked" => hex_color(kd::RED),
        _ => hex_color(kd::FG_MUTED),
    }
}

pub fn protondb_tier_title(tier: &str) -> String {
    let t = tier.trim();
    if t.is_empty() {
        return "Unknown".to_string();
    }
    let mut chars = t.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

pub fn open_umu_lookup_dialog(
    app_entity: Entity<UmuBoxApp>,
    initial_query: String,
    title: String,
    on_apply: Rc<dyn Fn(String, &mut Window, &mut App)>,
    window: &mut Window,
    cx: &mut App,
) {
    let query_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Search game title, Steam AppID, codename, or .exe…")
            .default_value(initial_query)
    });
    let selected_entry: Rc<RefCell<Option<UmuDbEntry>>> = Rc::new(RefCell::new(None));
    let protondb_state: Rc<RefCell<Option<(u64, Result<ProtonDbSummary, String>)>>> =
        Rc::new(RefCell::new(None));
    let protondb_fetching: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            query_input.focus_handle(cx).focus(window, cx);
        }

        let (umu_db, protons, umu_refreshing) = {
            let app = app_entity.read(cx);
            (
                app.umu_db.clone(),
                app.installed_protons.clone(),
                app.umu_db_refreshing,
            )
        };

        let q = query_input.read(cx).value().trim().to_string();
        let matches: Vec<UmuDbEntry> = search_umu_database(&umu_db, &q, 60)
            .into_iter()
            .cloned()
            .collect();

        if selected_entry
            .borrow()
            .as_ref()
            .is_none_or(|cur| !matches.iter().any(|m| m.umu_id == cur.umu_id && m.store == cur.store))
        {
            *selected_entry.borrow_mut() = matches.first().cloned();
        }

        let current_sel = selected_entry.borrow().clone();

        // Trigger async ProtonDB summary fetch when selected entry maps to a Steam AppID.
        if let Some(ref entry) = current_sel
            && let Some(app_id) = extract_steam_appid(&entry.umu_id)
        {
            let already_loaded = protondb_state
                .borrow()
                .as_ref()
                .is_some_and(|(id, _)| *id == app_id);
            if !already_loaded && protondb_fetching.get() != Some(app_id) {
                protondb_fetching.set(Some(app_id));
                let pdb_st = protondb_state.clone();
                let pdb_fetch = protondb_fetching.clone();
                let win_h = window.window_handle();
                cx.spawn(async move |cx| {
                    let res = cx
                        .background_executor()
                        .spawn(async move {
                            fetch_protondb_summary(app_id).map_err(|e| e.to_string())
                        })
                        .await;
                    let _ = win_h.update(cx, |_, window, _| {
                        *pdb_st.borrow_mut() = Some((app_id, res));
                        if pdb_fetch.get() == Some(app_id) {
                            pdb_fetch.set(None);
                        }
                        window.refresh();
                    });
                })
                .detach();
            }
        }

        let app_for_refresh = app_entity.clone();
        let border = hex_color(kd::BORDER_SUBTLE);
        let panel_bg = hex_color(kd::BG_SIDEBAR);
        let card_bg = hex_color(kd::BG_PANEL);
        let blue = hex_color(kd::BLUE);
        let green = hex_color(kd::GREEN);

        let sel_id_for_apply = current_sel.as_ref().map(|e| e.umu_id.clone());
        let apply_cb = on_apply.clone();

        dialog
            .title(title.clone())
            .w(px(740.))
            .child(
                v_flex()
                    .gap_2p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div().flex_1().child(
                                    Input::new(&query_input)
                                        .id("umu-db-search")
                                        .small()
                                        .prefix(
                                            Icon::new(IconName::Search)
                                                .xsmall()
                                                .text_color(hex_color(kd::FG_MUTED)),
                                        )
                                        .w_full(),
                                ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_SUBTLE))
                                    .child(format!("{} / {} entries", matches.len(), umu_db.len())),
                            )
                            .child(
                                Button::new("umu-db-refresh")
                                    .small()
                                    .outline()
                                    .icon(IconName::RefreshCw)
                                    .label(if umu_refreshing {
                                        "Updating…"
                                    } else {
                                        "Refresh Online DB"
                                    })
                                    .disabled(umu_refreshing)
                                    .on_click(move |_, window, cx| {
                                        app_for_refresh.update(cx, |app, cx| {
                                            app.refresh_umu_db_online(window, cx);
                                        });
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("umu-db-results-scroll")
                            .h(px(230.))
                            .overflow_y_scrollbar()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(border)
                            .bg(panel_bg)
                            .p_1p5()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .when(matches.is_empty(), |col| {
                                        col.child(
                                            div()
                                                .p_3()
                                                .text_xs()
                                                .text_color(hex_color(kd::FG_MUTED))
                                                .child(if umu_db.is_empty() {
                                                    "No local UMU database found. Click 'Refresh Online DB' above to fetch umu-database.csv."
                                                } else {
                                                    "No UMU entries match your search."
                                                }),
                                        )
                                    })
                                    .children(matches.iter().enumerate().map(|(i, item)| {
                                        let is_sel = current_sel.as_ref().is_some_and(|s| {
                                            s.umu_id == item.umu_id && s.store == item.store
                                        });
                                        let has_fix =
                                            find_protonfix_for_game_id(&item.umu_id, &protons)
                                                .is_some();
                                        let sel_target = selected_entry.clone();
                                        let entry_clone = item.clone();
                                        let app_r = app_entity.clone();

                                        h_flex()
                                            .id(("umu-row", i))
                                            .w_full()
                                            .px_2p5()
                                            .py_1p5()
                                            .gap_2()
                                            .rounded(px(5.))
                                            .items_center()
                                            .justify_between()
                                            .cursor_pointer()
                                            .bg(if is_sel {
                                                accent_alpha(0x28)
                                            } else {
                                                card_bg
                                            })
                                            .border_1()
                                            .border_color(if is_sel { blue } else { border })
                                            .hover(|s| s.bg(hex_color(kd::BG_BTN)))
                                            .on_click(move |_, window, cx| {
                                                *sel_target.borrow_mut() = Some(entry_clone.clone());
                                                window.refresh();
                                                cx.notify(app_r.entity_id());
                                            })
                                            .child(
                                                h_flex()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(
                                                                gpui_kit::FontWeight::SEMIBOLD,
                                                            )
                                                            .text_color(hex_color(kd::FG))
                                                            .truncate()
                                                            .child(item.title.clone()),
                                                    )
                                                    .when(!item.store.is_empty(), |r| {
                                                        r.child(
                                                            div()
                                                                .px_1p5()
                                                                .py(px(1.))
                                                                .rounded(px(3.))
                                                                .bg(panel_bg)
                                                                .text_size(px(10.))
                                                                .text_color(hex_color(kd::FG_SUBTLE))
                                                                .flex_shrink_0()
                                                                .child(item.store.clone()),
                                                        )
                                                    })
                                                    .when(has_fix, |r| {
                                                        r.child(
                                                            div()
                                                                .px_1p5()
                                                                .py(px(1.))
                                                                .rounded(px(3.))
                                                                .bg(panel_bg)
                                                                .text_size(px(10.))
                                                                .font_weight(
                                                                    gpui_kit::FontWeight::SEMIBOLD,
                                                                )
                                                                .text_color(green)
                                                                .flex_shrink_0()
                                                                .child("Auto-Fix"),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_family("monospace")
                                                    .text_color(hex_color(kd::AQUA))
                                                    .flex_shrink_0()
                                                    .child(item.umu_id.clone()),
                                            )
                                    })),
                            ),
                    )
                    // Selected Entry Details: Protonfixes + ProtonDB
                    .when_some(current_sel, |col, entry| {
                        let pfix = find_protonfix_for_game_id(&entry.umu_id, &protons);
                        let steam_id = extract_steam_appid(&entry.umu_id);
                        let pdb_res = steam_id.and_then(|aid| {
                            protondb_state
                                .borrow()
                                .as_ref()
                                .filter(|(id, _)| *id == aid)
                                .map(|(_, r)| r.clone())
                        });

                        col.child(
                            v_flex()
                                .w_full()
                                .p_3()
                                .gap_2()
                                .rounded(px(6.))
                                .bg(card_bg)
                                .border_1()
                                .border_color(border)
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .items_center()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(gpui_kit::FontWeight::BOLD)
                                                        .text_color(hex_color(kd::FG))
                                                        .child(entry.title.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .px_1p5()
                                                        .py(px(1.))
                                                        .rounded(px(4.))
                                                        .bg(panel_bg)
                                                        .text_xs()
                                                        .font_family("monospace")
                                                        .text_color(hex_color(kd::AQUA))
                                                        .child(entry.umu_id.clone()),
                                                ),
                                        )
                                        .when_some(steam_id, |row, aid| {
                                            row.child(
                                                Button::new("open-pdb-web")
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(IconName::ExternalLink)
                                                    .label("ProtonDB")
                                                    .on_click(move |_, _, _| {
                                                        let _ =
                                                            open_url_in_browser(&protondb_url(aid));
                                                    }),
                                            )
                                        }),
                                )
                                .when(!entry.note.is_empty(), |box_c| {
                                    box_c.child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_SUBTLE))
                                            .child(format!("Note: {}", entry.note)),
                                    )
                                })
                                .child(if let Some(fix) = pfix {
                                    let mut details = Vec::new();
                                    if !fix.detected_verbs.is_empty() {
                                        details.push(format!(
                                            "Auto-verbs: {}",
                                            fix.detected_verbs.join(" ")
                                        ));
                                    }
                                    if !fix.detected_dll_overrides.is_empty() {
                                        details.push(format!(
                                            "Auto-DLLs: {}",
                                            fix.detected_dll_overrides.join("; ")
                                        ));
                                    }
                                    let detail_text = if details.is_empty() {
                                        format!("Bundled fix ({}) runs automatically on launch", fix.store_bucket)
                                    } else {
                                        format!("Bundled fix ({}): {}", fix.store_bucket, details.join(" · "))
                                    };
                                    v_flex()
                                        .gap_0p5()
                                        .child(
                                            h_flex()
                                                .gap_1p5()
                                                .items_center()
                                                .child(
                                                    Icon::new(IconName::BadgeCheck)
                                                        .xsmall()
                                                        .text_color(green),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                        .text_color(green)
                                                        .child(detail_text),
                                                ),
                                        )
                                        .when(!fix.summary.is_empty(), |c| {
                                            c.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(hex_color(kd::FG_MUTED))
                                                    .child(fix.summary),
                                            )
                                        })
                                } else {
                                    v_flex().child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child("Protonfixes: No custom gamefix needed (standard Proton runtime)."),
                                    )
                                })
                                .when_some(steam_id, |box_c, _| {
                                    let pdb_line = match pdb_res {
                                        Some(Ok(ref s)) => h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py(px(1.))
                                                    .rounded(px(4.))
                                                    .bg(panel_bg)
                                                    .text_xs()
                                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                                    .text_color(protondb_tier_color(&s.tier))
                                                    .child(format!(
                                                        "ProtonDB: {}",
                                                        protondb_tier_title(&s.tier)
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(hex_color(kd::FG_SUBTLE))
                                                    .child(format!(
                                                        "score {:.2} · confidence: {} · {} reports (trending: {})",
                                                        s.score,
                                                        s.confidence,
                                                        s.total,
                                                        protondb_tier_title(&s.trending_tier)
                                                    )),
                                            )
                                            .into_any_element(),
                                        Some(Err(ref err)) => div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child(format!("ProtonDB: {err}"))
                                            .into_any_element(),
                                        None => div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child("Checking ProtonDB rating…")
                                            .into_any_element(),
                                    };
                                    box_c.child(pdb_line)
                                }),
                        )
                    }),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("umu-dlg-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("umu-dlg-apply")
                            .primary()
                            .icon(IconName::Check)
                            .label("Use GAMEID")
                            .disabled(sel_id_for_apply.is_none())
                            .on_click(move |_, window, cx| {
                                if let Some(gid) = sel_id_for_apply.clone() {
                                    window.close_dialog(cx);
                                    apply_cb(gid, window, cx);
                                }
                            }),
                    ),
            )
    });
}

pub fn open_dep_scanner_dialog(
    app_entity: Entity<UmuBoxApp>,
    pfx_idx: usize,
    game_idx: Option<usize>,
    initial_exe: Option<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    let initial_report: Option<Result<DependencyReport, String>> = {
        let pfx_path = app_entity
            .read(cx)
            .config
            .prefixes
            .get(pfx_idx)
            .map(|p| p.path.clone());
        initial_exe
            .as_ref()
            .filter(|p| p.is_file())
            .map(|exe| analyze_exe_dependencies(exe, pfx_path.as_deref()).map_err(|e| e.to_string()))
    };

    let report_state: Rc<RefCell<Option<Result<DependencyReport, String>>>> =
        Rc::new(RefCell::new(initial_report));

    window.open_dialog(cx, move |dialog, _, cx| {
        let (pfx_name, pfx_path) = {
            let app = app_entity.read(cx);
            app.config
                .prefixes
                .get(pfx_idx)
                .map(|p| (p.name.clone(), p.path.clone()))
                .unwrap_or_else(|| ("Prefix".into(), PathBuf::new()))
        };
        let installed_verbs = read_installed_winetricks_verbs(&pfx_path);

        let cur_report = report_state.borrow().clone();
        let border = hex_color(kd::BORDER_SUBTLE);
        let panel_bg = hex_color(kd::BG_SIDEBAR);
        let card_bg = hex_color(kd::BG_PANEL);
        let green = hex_color(kd::GREEN);
        let yellow = hex_color(kd::YELLOW);
        let blue = hex_color(kd::BLUE);

        let rep_for_pick = report_state.clone();
        let pfx_for_pick = pfx_path.clone();
        let app_for_pick = app_entity.clone();

        dialog
            .title(format!("Auto-Fix DLLs & Prefix Presets · '{pfx_name}'"))
            .w(px(740.))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .p_2p5()
                            .rounded(px(6.))
                            .bg(panel_bg)
                            .border_1()
                            .border_color(border)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(hex_color(kd::FG))
                                            .child("PE DLL Import & Mod Hook Scanner"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .truncate()
                                            .child(match &cur_report {
                                                Some(Ok(r)) => format!(
                                                    "Scanned {} ({} binary/ies, {} imported DLLs)",
                                                    r.target_exe
                                                        .file_name()
                                                        .and_then(|n| n.to_str())
                                                        .unwrap_or("exe"),
                                                    r.scanned_binaries,
                                                    r.imported_dlls.len()
                                                ),
                                                Some(Err(e)) => format!("Scan error: {e}"),
                                                None => {
                                                    "Pick a Windows .exe to detect required MSVC/.NET/DirectX runtimes & mod DLLs."
                                                        .to_string()
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                Button::new("dep-scan-pick-exe")
                                    .small()
                                    .outline()
                                    .icon(IconName::FolderOpen)
                                    .label("Scan .exe…")
                                    .on_click(move |_, window, cx| {
                                        let rx = cx.prompt_for_paths(PathPromptOptions {
                                            files: true,
                                            directories: false,
                                            multiple: false,
                                            prompt: Some("Select Windows .exe to Scan".into()),
                                        });
                                        let rep_st = rep_for_pick.clone();
                                        let pfx_p = pfx_for_pick.clone();
                                        let app_r = app_for_pick.clone();
                                        window
                                            .spawn(cx, async move |cx| {
                                                if let Ok(Ok(Some(paths))) = rx.await
                                                    && let Some(exe) = paths.into_iter().next()
                                                {
                                                    let res = analyze_exe_dependencies(
                                                        &exe,
                                                        Some(&pfx_p),
                                                    )
                                                    .map_err(|e| e.to_string());
                                                    *rep_st.borrow_mut() = Some(res);
                                                    let _ = cx.update(|window, cx| {
                                                        window.refresh();
                                                        cx.notify(app_r.entity_id());
                                                    });
                                                }
                                            })
                                            .detach();
                                    }),
                            ),
                    )
                    // Detected Recommendations & Side-by-Side Overrides
                    .when_some(cur_report.and_then(Result::ok), |col, rep| {
                        let missing_verbs: Vec<String> = rep
                            .recommendations
                            .iter()
                            .filter(|r| !r.already_installed)
                            .map(|r| r.verb.clone())
                            .collect();
                        let sbs_specs: Vec<String> = rep
                            .side_by_side_overrides
                            .iter()
                            .map(|s| s.override_spec.clone())
                            .collect();

                        let app_install = app_entity.clone();
                        let app_sbs = app_entity.clone();

                        col.child(
                            v_flex()
                                .gap_2()
                                .p_3()
                                .rounded(px(6.))
                                .bg(card_bg)
                                .border_1()
                                .border_color(border)
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .items_center()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui_kit::FontWeight::BOLD)
                                                .text_color(hex_color(kd::FG))
                                                .child(format!(
                                                    "Detected Runtime Dependencies ({})",
                                                    rep.recommendations.len()
                                                )),
                                        )
                                        .when(!missing_verbs.is_empty(), |r| {
                                            let mv = missing_verbs.clone();
                                            r.child(
                                                Button::new("dep-install-missing")
                                                    .primary()
                                                    .xsmall()
                                                    .icon(IconName::Play)
                                                    .label(format!(
                                                        "Install Missing ({})",
                                                        mv.len()
                                                    ))
                                                    .on_click(move |_, window, cx| {
                                                        let verbs = mv.clone();
                                                        window.close_dialog(cx);
                                                        app_install.update(cx, |app, cx| {
                                                            app.run_winetricks_verbs(
                                                                pfx_idx, verbs, true, window, cx,
                                                            );
                                                        });
                                                    }),
                                            )
                                        }),
                                )
                                .when(rep.recommendations.is_empty(), |c| {
                                    c.child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child("No extra MSVC/DirectX/.NET runtime verbs flagged in PE import table."),
                                    )
                                })
                                .children(rep.recommendations.iter().enumerate().map(|(i, rec)| {
                                    h_flex()
                                        .id(("dep-rec", i))
                                        .justify_between()
                                        .items_center()
                                        .px_2()
                                        .py_1()
                                        .rounded(px(4.))
                                        .bg(panel_bg)
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w_0()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(
                                                            gpui_kit::FontWeight::SEMIBOLD,
                                                        )
                                                        .text_color(hex_color(kd::FG))
                                                        .child(format!(
                                                            "{} (`{}`)",
                                                            rec.label, rec.verb
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(hex_color(kd::FG_MUTED))
                                                        .truncate()
                                                        .child(format!(
                                                            "{} · DLLs: {}",
                                                            rec.reason,
                                                            rec.matched_dlls.join(", ")
                                                        )),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui_kit::FontWeight::BOLD)
                                                .text_color(if rec.already_installed {
                                                    green
                                                } else {
                                                    yellow
                                                })
                                                .flex_shrink_0()
                                                .child(if rec.already_installed {
                                                    "Installed ✓"
                                                } else {
                                                    "Missing"
                                                }),
                                        )
                                }))
                                .when(!rep.side_by_side_overrides.is_empty(), |c| {
                                    let specs = sbs_specs.clone();
                                    c.child(
                                        v_flex()
                                            .gap_1p5()
                                            .pt_2()
                                            .border_t_1()
                                            .border_color(border)
                                            .child(
                                                h_flex()
                                                    .justify_between()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(gpui_kit::FontWeight::BOLD)
                                                            .text_color(blue)
                                                            .child("Side-by-Side Mod / Hook DLLs Detected"),
                                                    )
                                                    .child(
                                                        Button::new("dep-apply-sbs")
                                                            .xsmall()
                                                            .outline()
                                                            .icon(IconName::Check)
                                                            .label("Apply WINEDLLOVERRIDES")
                                                            .on_click(move |_, window, cx| {
                                                                let add = specs.clone();
                                                                app_sbs.update(cx, |app, cx| {
                                                                    if let Some(gi) = game_idx {
                                                                        app.apply_dll_overrides_to_game(
                                                                            pfx_idx, gi, add, window, cx,
                                                                        );
                                                                    } else {
                                                                        app.apply_dll_overrides_to_prefix(
                                                                            pfx_idx, add, window, cx,
                                                                        );
                                                                    }
                                                                });
                                                            }),
                                                    ),
                                            )
                                            .children(rep.side_by_side_overrides.iter().map(|s| {
                                                div()
                                                    .text_xs()
                                                    .text_color(hex_color(kd::FG_SUBTLE))
                                                    .child(format!(
                                                        "• {} → {}",
                                                        s.filename, s.override_spec
                                                    ))
                                            })),
                                    )
                                }),
                        )
                    })
                    // Curated Prefix Presets
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                    .text_color(hex_color(kd::FG))
                                    .child("One-Click Curated Prefix Presets (`winetricks -q`)"),
                            )
                            .children(PREFIX_PRESETS.iter().enumerate().map(|(pi, preset)| {
                                let missing_in_preset: Vec<String> = preset
                                    .verbs
                                    .iter()
                                    .filter(|v| !installed_verbs.contains(&v.to_ascii_lowercase()))
                                    .map(|v| (*v).to_string())
                                    .collect();
                                let all_verbs: Vec<String> =
                                    preset.verbs.iter().map(|s| (*s).to_string()).collect();
                                let done_cnt = preset.verbs.len() - missing_in_preset.len();
                                let app_pre = app_entity.clone();

                                h_flex()
                                    .id(("preset-card", pi))
                                    .w_full()
                                    .p_2p5()
                                    .gap_2()
                                    .rounded(px(6.))
                                    .bg(card_bg)
                                    .border_1()
                                    .border_color(border)
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_0p5()
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(
                                                                gpui_kit::FontWeight::BOLD,
                                                            )
                                                            .text_color(hex_color(kd::FG))
                                                            .child(preset.name),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(10.))
                                                            .text_color(
                                                                if missing_in_preset.is_empty() {
                                                                    green
                                                                } else {
                                                                    hex_color(kd::FG_MUTED)
                                                                },
                                                            )
                                                            .child(format!(
                                                                "{done_cnt}/{} logged",
                                                                preset.verbs.len()
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(hex_color(kd::FG_MUTED))
                                                    .child(format!(
                                                        "{} — [{}]",
                                                        preset.description,
                                                        preset.verbs.join(", ")
                                                    )),
                                            ),
                                    )
                                    .child(
                                        Button::new(("preset-run", pi))
                                            .xsmall()
                                            .outline()
                                            .icon(IconName::Play)
                                            .label(if missing_in_preset.is_empty() {
                                                "Re-run (-q)"
                                            } else {
                                                "Install (-q)"
                                            })
                                            .on_click(move |_, window, cx| {
                                                let verbs = if missing_in_preset.is_empty() {
                                                    all_verbs.clone()
                                                } else {
                                                    missing_in_preset.clone()
                                                };
                                                let dlls = preset.dll_overrides;
                                                window.close_dialog(cx);
                                                app_pre.update(cx, |app, cx| {
                                                    if !dlls.is_empty() {
                                                        let parts: Vec<String> = dlls
                                                            .split(';')
                                                            .map(|s| s.trim().to_string())
                                                            .filter(|s| !s.is_empty())
                                                            .collect();
                                                        app.apply_dll_overrides_to_prefix(
                                                            pfx_idx, parts, window, cx,
                                                        );
                                                    }
                                                    app.run_winetricks_verbs(
                                                        pfx_idx, verbs, true, window, cx,
                                                    );
                                                });
                                            }),
                                    )
                            })),
                    ),
            )
            .footer(
                DialogFooter::new().child(
                    Button::new("dep-close")
                        .label("Close")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}
