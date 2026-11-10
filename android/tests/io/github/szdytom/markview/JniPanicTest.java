package io.github.szdytom.markview;

public class JniPanicTest extends BridgeTest {
    @Override protected void test() throws Exception {
        checkFatal(3, "Injected JNI panic");
    }
}
