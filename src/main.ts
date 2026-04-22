import { createApp } from "vue";
import { createPinia } from "pinia";
import ElementPlus from "element-plus";
import "element-plus/dist/index.css";
import * as ElementPlusIconsVue from "@element-plus/icons-vue";
import App from "./App.vue";

const app = createApp(App);

// 注册 Element Plus 图标
for (const [key, component] of Object.entries(ElementPlusIconsVue)) {
  app.component(key, component);
}

// 使用 Pinia 状态管理
app.use(createPinia());

app.use(ElementPlus);

app.mount("#app");

const style = document.createElement("style");
style.textContent = `
* {
  scrollbar-width: none !important;
  -ms-overflow-style: none !important;
}
*::-webkit-scrollbar {
  display: none !important;
  width: 0 !important;
  height: 0 !important;
}
html, body {
  overflow: hidden !important;
}

/* 兜底：隐藏 WebKit 自动填充相关按钮 */
input::-webkit-contacts-auto-fill-button,
input::-webkit-credentials-auto-fill-button,
input::-webkit-caps-lock-indicator {
  display: none !important;
  visibility: hidden !important;
  pointer-events: none !important;
}
`;
document.head.appendChild(style);

// 禁用 WKWebView 的输入历史建议气泡：给所有 input 加 autocomplete=off 等属性
function suppressInputSuggestions(root: ParentNode = document) {
  root.querySelectorAll<HTMLInputElement>("input, textarea").forEach((el) => {
    if (el.dataset.suggestionSuppressed === "1") return;
    el.setAttribute("autocomplete", "off");
    el.setAttribute("autocorrect", "off");
    el.setAttribute("autocapitalize", "off");
    el.setAttribute("spellcheck", "false");
    // 部分密码管理器/浏览器识别
    el.setAttribute("data-form-type", "other");
    el.setAttribute("data-lpignore", "true");
    el.dataset.suggestionSuppressed = "1";
  });
}

// 初次处理已有 DOM
suppressInputSuggestions();
// DOM 变化时持续处理（如对话框、动态表单）
const inputObserver = new MutationObserver((mutations) => {
  for (const m of mutations) {
    m.addedNodes.forEach((node) => {
      if (node.nodeType === Node.ELEMENT_NODE) {
        suppressInputSuggestions(node as Element);
      }
    });
  }
});
inputObserver.observe(document.body, { childList: true, subtree: true });
