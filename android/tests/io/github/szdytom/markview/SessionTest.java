package io.github.szdytom.markview;

import android.app.Activity;
import android.content.Intent;
import android.os.SystemClock;
import android.view.KeyEvent;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import org.json.JSONObject;

public class SessionTest extends AndroidTest {
    @Override protected void test() throws Exception {
        File directory = new File(getTargetContext().getFilesDir(), "markview");
        File session = new File(directory, "session.json");
        File expectedFile = new File(directory, "session-test.json");
        if (arguments.getString("session-phase").equals("seed")) {
            directory.mkdirs();
            session.delete();
            Files.write(new File(directory, "settings.toml").toPath(), "version = 1\nstyle = [\"light\"]\n".getBytes(StandardCharsets.UTF_8));
            activity = startActivitySync(intent("reader.md", Intent.ACTION_VIEW));
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md") && loadedImages(s) >= 2);
            swipe(180, 590, 180, 180);
            waitFor(s -> s.optDouble("scroll") > 100);
            swipe(180, 300, 180, 300);
            stableLayout();
            Activity previous = activity;
            getTargetContext().startActivity(intent("second.md", Intent.ACTION_VIEW));
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("second.md"));
            getTargetContext().startActivity(intent("reader.md", Intent.ACTION_VIEW));
            JSONObject expected = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md") && loadedImages(s) >= 2);
            expected = stableLayout();
            require(activity == previous && !previous.isDestroyed() && expected.getJSONArray("tabs").length() == 2, "External opens reuse the singleTask Activity");
            require(expected.getBoolean("restore_session") && expected.getBoolean("single_instance") && !expected.getBoolean("instance_listener"), "Android defaults enable session restore without desktop IPC");
            Files.write(expectedFile.toPath(), expected.toString().getBytes(StandardCharsets.UTF_8));
            sendKeyDownUpSync(KeyEvent.KEYCODE_HOME);
            long deadline = SystemClock.uptimeMillis() + 5000;
            while ((!session.exists() || new JSONObject(new String(Files.readAllBytes(session.toPath()), StandardCharsets.UTF_8)).getJSONArray("tabs").length() != 2) && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50);
            require(session.exists(), "Background saves the session");
            JSONObject saved = new JSONObject(new String(Files.readAllBytes(session.toPath()), StandardCharsets.UTF_8));
            require(saved.getJSONArray("tabs").length() == 2 && saved.getInt("active") == expected.getInt("active"), "Background saves all tabs and the active tab");
            pass("Android background persistence and singleTask external opens");
        } else {
            JSONObject expected = new JSONObject(new String(Files.readAllBytes(expectedFile.toPath()), StandardCharsets.UTF_8));
            activity = startActivitySync(new Intent(getTargetContext(), MarkviewActivity.class).setAction(Intent.ACTION_MAIN).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            JSONObject restored = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md") && loadedImages(s) >= 2);
            restored = stableLayout();
            require(restored.getJSONArray("tabs").toString().equals(expected.getJSONArray("tabs").toString()) && restored.getInt("active") == expected.getInt("active"), "Process restart restores tab order and current tab");
            require(Math.abs(restored.getDouble("scroll") - expected.getDouble("scroll")) < 2, "Process restart restores the reading position: " + expected.getDouble("scroll") + " -> " + restored.getDouble("scroll"));
            getTargetContext().startActivity(intent("reader.md", Intent.ACTION_VIEW));
            JSONObject opened = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md") && loadedImages(s) >= 2);
            require(opened.getJSONArray("tabs").length() == 2, "External opening selects the restored tab without duplicates");
            pass("Android process restart restores tabs and reading positions");
        }
    }
}
