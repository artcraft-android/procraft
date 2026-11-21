package io.github.artcraftandroid.procraft

import org.junit.Assert.assertEquals
import org.junit.Test

/** 显示层翻译的冒烟测试：表、规则、以及不该被翻的字符串。 */
class ZhTest {
    @Test
    fun exactEntriesAreTranslated() {
        assertEquals("完成", Zh.t("Done"))
        assertEquals("图层", Zh.t("Layers"))
        assertEquals("画笔工作室", Zh.t("Brush Studio"))
        assertEquals("湿混色", Zh.t("Wet Mix"))
        assertEquals("此操作无法撤销。", Zh.t("This can't be undone."))
        assertEquals("无法打开此文件。", Zh.t("This file can't be opened."))
        // 调整菜单（Rust 侧给出名字）：
        assertEquals("曲线", Zh.t("Curves"))
        assertEquals("高斯模糊", Zh.t("Gaussian Blur"))
        assertEquals("色差", Zh.t("Chromatic Aberration"))
        assertEquals("液化", Zh.t("Liquify"))
    }

    @Test
    fun messagesWithNamesFollowThePatterns() {
        assertEquals("“图层 3” 已锁定，无法更改。", Zh.t("“图层 3” can't be changed while it is locked."))
        assertEquals("删除“我的作品”？", Zh.t("Delete “我的作品”?"))
        assertEquals("其 5 幅作品会一并删除。此操作无法撤销。", Zh.t("Its 5 artworks go with it. This can't be undone."))
        assertEquals("阈值 45%", Zh.t("Threshold 45%"))
        assertEquals("阈值 12.5%", Zh.t("Threshold 12.5%"))
        assertEquals("第 12 帧", Zh.t("Frame 12"))
        assertEquals("版本 1.2.3，构建 456", Zh.t("Version 1.2.3 Build 456"))
        assertEquals("已吸附到 圆", Zh.t("Snapped to a circle"))
    }

    @Test
    fun nonInterfaceStringsAreLeftAlone() {
        assertEquals("", Zh.t(""))
        assertEquals("45%", Zh.t("45%"))
        assertEquals("120 px", Zh.t("120 px"))
        assertEquals("image/png", Zh.t("image/png"))
        assertEquals("Unlisted phrase", Zh.t("Unlisted phrase"))
        assertEquals("图层", Zh.t("图层"))
    }

    @Test
    fun everyValueIsChinese() {
        // 表里不该有“译文等于原文”的漏翻
        val untranslated = listOf("Done", "Layers", "Brush", "Canvas", "Gallery")
            .filter { Zh.t(it) == it }
        assertEquals(emptyList<String>(), untranslated)
    }
}
