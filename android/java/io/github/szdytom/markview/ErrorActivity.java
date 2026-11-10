package io.github.szdytom.markview;

import android.app.Activity;
import android.os.Bundle;

/** A fatal error outlives the `NativeActivity` that the glue finishes. */
public final class ErrorActivity extends Activity {
    private android.app.AlertDialog dialog;
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        try { dialog = ErrorDialog.show(this, getIntent().getStringExtra("details"), true); }
        catch (RuntimeException error) {
            android.util.Log.e("Markview", "Cannot display fatal error", error);
            finish();
        }
    }
    @Override public void onDestroy() {
        if (dialog != null) dialog.dismiss();
        super.onDestroy();
    }
}
