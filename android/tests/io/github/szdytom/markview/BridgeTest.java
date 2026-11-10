package io.github.szdytom.markview;

import android.os.SystemClock;
import android.view.accessibility.AccessibilityNodeInfo;
import java.lang.reflect.Method;

public class BridgeTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        guarded(() -> MarkviewActivity.nativeSharedUrl(null));
        expectError("Read shared text");
        dismissError();
        guarded(() -> MarkviewActivity.nativeTestFailure(0));
        expectError("Injected Rust-to-Java call");
        expectError("java.lang.IllegalStateException: Injected Java failure");
        expectError("Stack trace:");
        captureScreenshot("java-exception");
        dismissError();
        guarded(() -> MarkviewActivity.nativeTestFailure(1));
        expectError("Injected string allocation failure");
        dismissError();
        guarded(() -> MarkviewActivity.nativeTestFailure(2));
        require(MarkviewActivity.nativeBridgeReferencesReleased(), "JNI references are released after failures");
        waitFor(s -> s.optBoolean("ready"));
        pass("JNI errors preserve context and Java stacks, release references and cancel pending requests without holding callback locks");

        guarded(() -> MarkviewActivity.nativeTestFailure(5));
        guarded(0, () -> { throw new IllegalStateException("Injected picker launch failure"); });
        expectError("Injected picker launch failure");
        guarded(1, () -> { throw new IllegalStateException("Injected export launch failure"); });
        expectError("Injected export launch failure");
        dismissError();
        guarded(() -> MarkviewActivity.nativeTestFailure(6));

        ((MarkviewActivity)activity).inputMenu("invalid bounds");
        expectError("Show input menu");
        captureScreenshot("async-error");
        dismissError();
        runOnMainSync(() -> {
            ((MarkviewActivity)activity).showError("Repeated test error", false);
            ((MarkviewActivity)activity).showError("Repeated test error", false);
            ((MarkviewActivity)activity).showError("Second test error", false);
        });
        expectError("Repeated test error");
        expectError("Second test error");
        require(errorText("Repeated test error").indexOf("Repeated test error")
            == errorText("Repeated test error").lastIndexOf("Repeated test error"), "Repeated errors are merged");
        expectError("API ");
        dismissError();
        tap("SearchOpen");
        waitFor(s -> s.optBoolean("search_open"));
        commitInput("reader");
        waitFor(s -> s.optString("search_query").equals("reader"));
        runOnMainSync(() -> activity.getCurrentFocus().onCreateInputConnection(new android.view.inputmethod.EditorInfo()).deleteSurroundingText(-1, -1));
        waitFor(s -> s.optString("search_query").equals("reader"));
        runOnMainSync(() -> activity.getCurrentFocus().onCreateInputConnection(new android.view.inputmethod.EditorInfo()).deleteSurroundingTextInCodePoints(Integer.MAX_VALUE, Integer.MAX_VALUE));
        waitFor(s -> s.optString("search_query").isEmpty());
        pass("Asynchronous errors use system dialogs, merge duplicates and leave the reader usable");
    }

    protected void guarded(Runnable action) throws Exception {
        guarded(-1, action);
    }

    private void guarded(int kind, Runnable action) throws Exception {
        Method method = MarkviewActivity.class.getDeclaredMethod("attempt", String.class, int.class, Runnable.class);
        method.setAccessible(true);
        runOnMainSync(() -> {
            try { method.invoke(activity, "Test native operation", kind, action); }
            catch (ReflectiveOperationException error) { throw new AssertionError(error); }
        });
    }

    protected String errorText(String text) {
        AccessibilityNodeInfo root = getUiAutomation().getRootInActiveWindow();
        if (root != null) for (AccessibilityNodeInfo node : root.findAccessibilityNodeInfosByText(text)) {
            if (node.getText() != null) return node.getText().toString();
        }
        return "";
    }

    protected void expectError(String text) {
        long deadline = SystemClock.uptimeMillis() + 10000;
        while (SystemClock.uptimeMillis() < deadline) {
            if (!errorText(text).isEmpty()) return;
            SystemClock.sleep(100);
        }
        throw new AssertionError("Error dialog contains: " + text);
    }

    protected void dismissError() throws Exception {
        clickText(activity.getString(android.R.string.ok));
        waitForIdleSync();
    }

    protected void checkFatal(int kind, String text) throws Exception {
        openReader();
        int pid = android.os.Process.myPid();
        ActivityMonitor monitor = addMonitor(ErrorActivity.class.getName(), null, false);
        try {
            guarded(() -> MarkviewActivity.nativeTestFailure(kind));
            android.app.Activity errorActivity = monitor.waitForActivityWithTimeout(10000);
            require(errorActivity != null, "Fatal error starts a Java-only activity");
            long deadline = SystemClock.uptimeMillis() + 10000;
            while (!activity.isDestroyed() && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50);
            require(activity.isDestroyed(), "Fatal error destroys the reader");
            require(android.os.Process.myPid() == pid, "JNI panic does not abort the process");
            expectError(text);
            expectError("API ");
            require(!errorActivity.isFinishing(), "Fatal dialog survives reader shutdown");
            activity = errorActivity;
            captureScreenshot("fatal-error");
            dismissError();
            pass("Fatal error ends the reader and remains visible without Rust or a GPU renderer");
        } finally { removeMonitor(monitor); }
    }
}
