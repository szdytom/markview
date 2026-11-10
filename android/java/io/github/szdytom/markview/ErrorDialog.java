package io.github.szdytom.markview;

import android.app.Activity;
import android.app.AlertDialog;
import android.os.Build;
import android.widget.TextView;

/** System widgets keep errors readable without the native renderer. */
final class ErrorDialog {
    static AlertDialog show(Activity activity, String details, boolean fatal) {
        String version;
        try { version = activity.getPackageManager().getPackageInfo(activity.getPackageName(), 0).versionName; }
        catch (android.content.pm.PackageManager.NameNotFoundException error) { throw new AssertionError(error); }
        String header = "Markview " + version + "\n" + Build.MANUFACTURER + " " + Build.MODEL
            + " · Android " + Build.VERSION.RELEASE + " (API " + Build.VERSION.SDK_INT + ")"
            + " · " + String.join(", ", Build.SUPPORTED_ABIS);
        AlertDialog dialog = new AlertDialog.Builder(activity)
            .setTitle(fatal ? "Markview stopped" : "Markview error")
            .setMessage(header + "\n\n" + details)
            .setPositiveButton(android.R.string.ok, (ignored, button) -> { if (fatal) activity.finish(); })
            .setCancelable(!fatal)
            .create();
        dialog.show();
        ((TextView)dialog.findViewById(android.R.id.message)).setTextIsSelectable(true);
        return dialog;
    }
}
