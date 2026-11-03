use super::*;

/// One settled frame of `draw` at 400x300 points, described.
fn described(draw: impl Fn(&mut egui::Ui)) -> String {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let raw = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400., 300.))),
        ..Default::default()
    };
    ctx.run_ui(raw(), |ui| draw(ui)).textures_delta.clear();
    let mut output = ctx.run_ui(raw(), |ui| draw(ui));
    output.textures_delta.clear();
    describe(&ctx, &output)
}

#[test]
fn widgets_carry_their_labels_places_and_states() {
    let text = described(|ui| {
        ui.add_enabled(false, egui::Button::new("Found city"));
        ui.checkbox(&mut true.clone(), "Grid");
        ui.label("Turn 42");
    });
    let widgets = text.split("painted\n").next().unwrap();
    assert!(widgets.contains("button \"Found city\""), "{text}");
    assert!(widgets.contains(" disabled"), "{text}");
    assert!(widgets.contains("checkbox \"Grid\"") && widgets.contains(" on"), "{text}");
    assert!(!widgets.contains("Turn 42"), "labels are painted texts, not widgets:\n{text}");
    assert!(text.contains("text \"Turn 42\""), "{text}");
}

#[test]
fn a_clickable_area_without_a_label_is_named_by_the_texts_inside_it() {
    let text = described(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(120., 40.), egui::Sense::click());
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Research", egui::FontId::proportional(14.), egui::Color32::WHITE);
    });
    assert!(text.contains("click \"Research\""), "{text}");
}

#[test]
fn shapes_list_rounded_places_and_colors_and_skip_what_does_not_show() {
    let text = described(|ui| {
        let painter = ui.painter();
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(10.4, 20.6), egui::vec2(100., 50.)), 4.0, egui::Color32::from_rgb(10, 20, 30));
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(0., 0.), egui::vec2(5., 5.)), 0.0, egui::Color32::TRANSPARENT);
        painter.circle_filled(egui::pos2(-50., -50.), 4., egui::Color32::RED);
    });
    assert!(text.contains("rect 10,21 100x50 fill #0a141e r4"), "{text}");
    assert_eq!(text.matches("rect ").count(), 1, "the transparent rect is not listed:\n{text}");
    assert!(!text.contains("circle"), "the circle is off screen:\n{text}");
}

#[test]
fn the_same_frame_describes_the_same_twice() {
    let draw = |ui: &mut egui::Ui| {
        ui.heading("Research");
        let _ = ui.button("Back");
    };
    assert_eq!(described(draw), described(draw));
}

#[test]
fn a_matching_snapshot_passes_a_changed_one_fails_with_a_diff_and_update_rewrites_it() {
    let folder = std::env::temp_dir().join(format!("shell-snapshot-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let path = folder.join("nested").join("frame.snap");
    let kept = "screen Menu\n  text \"New game\"\n";
    let now = "screen Menu\n  text \"Continue\"\n";
    let missing = compare(&path, kept, false).expect_err("no snapshot yet");
    assert!(missing.contains("no snapshot") && missing.contains(UPDATE_VARIABLE), "{missing}");
    compare(&path, kept, true).expect("update writes the file and its folder");
    compare(&path, kept, false).expect("the same frame passes");
    let changed = compare(&path, now, false).expect_err("a changed frame fails");
    assert!(changed.contains("-   text \"New game\"\n+   text \"Continue\""), "{changed}");
    compare(&path, now, true).expect("update accepts the change");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), now);
    let _ = std::fs::remove_dir_all(&folder);
}
