pub struct Demo {
    pub name: &'static str,
    pub color: u32,
    pub staged: String,
    pub changes: String,
}

pub fn projects() -> Vec<Demo> {
    vec![
        Demo {
            name: "hitSlop",
            color: 0xff8175,
            staged: CAPTURE
                .split("diff --git ")
                .skip(1)
                .filter(|block| {
                    !block.starts_with("a/src/capture.rs")
                        && !block.starts_with("a/tests/timeout_test.rs")
                })
                .map(|block| format!("diff --git {block}"))
                .collect(),
            changes: format!(
                "{}{}",
                CAPTURE
                    .split("diff --git ")
                    .skip(1)
                    .filter(|block| block.starts_with("a/src/capture.rs")
                        || block.starts_with("a/tests/timeout_test.rs"))
                    .map(|block| format!("diff --git {block}"))
                    .collect::<String>(),
                DURATION_CHANGE
            ),
        },
        Demo {
            name: "Slide",
            color: 0x74d9b5,
            staged: String::new(),
            changes: SLIDE.into(),
        },
    ]
}

const CAPTURE: &str = r#"diff --git a/src/capture.rs b/src/capture.rs
index 3b9eafe..aa48f01 100644
--- a/src/capture.rs
+++ b/src/capture.rs
@@ -12,7 +12,12 @@
 pub async fn capture(app: &AppHandle) -> Result<PathBuf> {
     let region = active_region(app)?;
-    let image = native::capture(app, region).await?;
+    let image = tokio::time::timeout(
+        Duration::from_secs(5),
+        native::capture(app, region),
+    )
+    .await
+    .context("Capture timed out")??;
     let path = capture_path(app)?;
     image.save(&path)?;
     Ok(path)
 }
diff --git a/src/errors.rs b/src/errors.rs
index a09eaff..a040ffd 100644
--- a/src/errors.rs
+++ b/src/errors.rs
@@ -1,4 +1,5 @@
 pub enum CaptureError {
     Unavailable,
+    Timeout,
     PermissionDenied,
 }
diff --git a/src/duration.rs b/src/duration.rs
index 80abcdef..90abcdef 100644
--- a/src/duration.rs
+++ b/src/duration.rs
@@ -1,3 +1,3 @@
 use std::time::Duration;
 
-pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
+pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
diff --git a/tests/capture_test.rs b/tests/capture_test.rs
new file mode 100644
--- /dev/null
+++ b/tests/capture_test.rs
@@ -0,0 +1,9 @@
+#[tokio::test]
+async fn reports_capture_timeout() {
+    let backend = HangingCapture::new();
+    let result = capture_with(backend).await;
+    assert!(result.is_err());
+    assert!(result.unwrap_err().to_string()
+        .contains("Capture timed out"));
+}
+
diff --git a/tests/timeout_test.rs b/tests/timeout_test.rs
index a01eaff..a045ffd 100644
--- a/tests/timeout_test.rs
+++ b/tests/timeout_test.rs
@@ -1,4 +1,4 @@
 #[test]
 fn timeout_is_bounded() {
-    assert_eq!(CAPTURE_TIMEOUT.as_secs(), 30);
+    assert_eq!(CAPTURE_TIMEOUT.as_secs(), 5);
 }
diff --git a/Cargo.toml b/Cargo.toml
index 2b3ca30..2b3ca31 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,3 +1,3 @@
 [dependencies]
 anyhow = "1"
-tokio = { version = "1", features = ["rt"] }
+tokio = { version = "1", features = ["rt", "time"] }
"#;

const SLIDE: &str = r#"diff --git a/src/slides/navigation.rs b/src/slides/navigation.rs
index 3b9eafe..aa48f01 100644
--- a/src/slides/navigation.rs
+++ b/src/slides/navigation.rs
@@ -1,9 +1,11 @@
 pub fn select_next(&mut self, cx: &mut Context<Self>) {
-    self.selected += 1;
+    self.selected = (self.selected + 1).min(self.slides.len().saturating_sub(1));
+    self.scroll.scroll_to_item(self.selected);
     cx.notify();
 }
 
 pub fn select_previous(&mut self, cx: &mut Context<Self>) {
-    self.selected -= 1;
+    self.selected = self.selected.saturating_sub(1);
+    self.scroll.scroll_to_item(self.selected);
     cx.notify();
 }
diff --git a/src/slides/mod.rs b/src/slides/mod.rs
index aa0eaff..aa40ffd 100644
--- a/src/slides/mod.rs
+++ b/src/slides/mod.rs
@@ -1,3 +1,4 @@
 pub struct Slides {
     selected: usize,
+    scroll: UniformListScrollHandle,
 }
"#;

const DURATION_CHANGE: &str = r#"diff --git a/src/duration.rs b/src/duration.rs
--- a/src/duration.rs
+++ b/src/duration.rs
@@ -1,3 +1,3 @@
 use std::time::Duration;
 
-pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
+pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);
"#;
