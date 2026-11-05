package io.github.szdytom.markview;

import android.os.Environment;
import java.io.File;

public abstract class FolderTestSupport extends AndroidTest {
    protected File directory, rawDirectory, mediaDirectory;

    protected void createFolders() throws Exception {
        File downloadsDirectory = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS);
        directory = new File(downloadsDirectory, "Markview integration");
        rawDirectory = new File(downloadsDirectory, "Markview raw integration");
        mediaDirectory = new File(downloadsDirectory, "Markview media integration");
        for (File folder : new File[]{directory, rawDirectory, mediaDirectory}) deleteTree(folder);
        require(directory.mkdirs() && rawDirectory.mkdirs() && mediaDirectory.mkdirs(), "Create shared-storage folders");
        require(!MarkviewActivity.assetDirectory(getTargetContext(), directory), "Shared folders reject asset imports");
        require(MarkviewActivity.assetDirectory(getTargetContext(), new File(activity.getFilesDir(), "markview/fonts")), "Private font folders accept asset imports");
        copyAsset("folder.md", new File(directory, "README.md"));
        copyAsset("second.md", new File(directory, "chapter.md"));
        copyAsset("logo.svg", new File(directory, "images/logo.svg"));
        copyAsset("folder.md", new File(rawDirectory, "README.md"));
        copyAsset("folder.md", new File(mediaDirectory, "README.md"));
    }

    @Override protected void cleanup() {
        for (File folder : new File[]{directory, rawDirectory, mediaDirectory}) {
            if (folder != null) deleteTree(folder);
        }
    }
}
