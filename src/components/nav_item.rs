use gpui::*;

pub struct NavItem {
    label: SharedString,
    _is_active: bool,
    _on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl NavItem {
    pub fn new(label: impl Into<SharedString>, is_active: bool) -> Self {
        Self {
            label: label.into(),
            _is_active: is_active,
            _on_click: None,
        }
    }

    #[allow(dead_code)]
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self._on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for NavItem {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .px_3()
            .py_1p5()
            .rounded_md()
            .child(
                div()
                    .text_sm()
                    .child(self.label),
            )
    }
}
