qrc!(qml_resources,
    "/" {
        "qml/AddPage.qml",
        "qml/App.qml",
        "qml/ContentImport.qml",
        "qml/CustomIconSelector.qml",
        "qml/EssentialSettings.qml",
        "qml/IconButton.qml",
        "qml/IconWebView.qml",
        "qml/IconSelector.qml",
        "qml/IconSelectItem.qml",
        "qml/InstallDialog.qml",
        "qml/KeyboardPlaceholder.qml",
        "qml/Main.qml",
        "qml/MainPage.qml",
        "qml/MiniBrowserBar.qml",
        "qml/MiniBrowserView.qml",
        "qml/OptionalSettings.qml",
        "qml/qmldir",
        "qml/ScreenshotArea.qml",
        "qml/ScreenshotAreaCorner.qml",
        "qml/ScreenshotDialog.qml",
        "qml/ScreenshotSelector.qml",
        "assets/icon.svg",
        "assets/webber_spider.svg",
    },
);

pub fn load() {
    qml_resources();
}
