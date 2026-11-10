package io.github.szdytom.markview;

public class ReaderPanicTest extends BridgeTest {
    @Override protected void test() throws Exception {
        checkFatal(7, "Injected reader thread panic");
    }
}
