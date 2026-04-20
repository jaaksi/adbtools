<script setup lang="ts">
import { onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useAuthStore } from '../stores/auth';
import { GOOGLE_AUTH_CONFIG, isGoogleAuthConfigured } from '../config/googleAuth';
import type { GoogleUserInfo } from '../config/googleAuth';

const authStore = useAuthStore();

let unlistenCallback: UnlistenFn | null = null;

// 启动 Google 认证流程：Rust 起本地回环服务器 → 浏览器完成授权 → 回调写回 app
async function openGoogleAuth() {
  try {
    // 先清理上一次可能遗留的监听
    if (unlistenCallback) {
      unlistenCallback();
      unlistenCallback = null;
    }

    const port = await invoke<number>('start_oauth_server');
    const redirectUri = `http://127.0.0.1:${port}`;

    // 监听 Rust 端回调（只处理一次）
    unlistenCallback = await listen<string>('oauth_callback', async (event) => {
      if (unlistenCallback) {
        unlistenCallback();
        unlistenCallback = null;
      }

      const params = new URLSearchParams(event.payload);
      const idToken = params.get('id_token');
      const accessToken = params.get('access_token');
      const error = params.get('error');

      if (error) {
        alert('Google 认证失败: ' + error);
        return;
      }

      // 优先用 id_token 本地解出用户信息
      if (idToken) {
        const userInfo = authStore.decodeCredential(idToken);
        if (userInfo) {
          // 头像通过 Rust 代理转成 data URL，绕开 webview 限流
          if (userInfo.picture) {
            try {
              userInfo.picture = await invoke<string>('fetch_image_as_data_url', {
                url: userInfo.picture,
              });
            } catch (e) {
              console.warn('头像代理下载失败，保留原始 URL:', e);
            }
          }
          authStore.setUser(userInfo);
        }
        authStore.setToken(idToken);
        return;
      }

      // 兜底：用 access_token 调 userinfo
      if (accessToken) {
        authStore.setToken(accessToken);
        try {
          const res = await fetch('https://www.googleapis.com/oauth2/v3/userinfo', {
            headers: { Authorization: `Bearer ${accessToken}` },
          });
          const u = await res.json();
          const info: GoogleUserInfo = {
            id: u.sub,
            email: u.email,
            name: u.name,
            picture: u.picture,
            givenName: u.given_name,
            familyName: u.family_name,
          };
          authStore.setUser(info);
        } catch (e) {
          console.error('Failed to fetch userinfo:', e);
        }
      }
    });

    const nonce = Math.random().toString(36).slice(2) + Date.now().toString(36);
    const authUrl =
      `https://accounts.google.com/o/oauth2/v2/auth` +
      `?client_id=${encodeURIComponent(GOOGLE_AUTH_CONFIG.clientId)}` +
      `&redirect_uri=${encodeURIComponent(redirectUri)}` +
      `&response_type=${encodeURIComponent('token id_token')}` +
      `&scope=${encodeURIComponent('openid email profile')}` +
      `&nonce=${nonce}` +
      `&prompt=select_account`;

    await invoke('open_url', { url: authUrl });
  } catch (error) {
    console.error('Failed to start OAuth:', error);
    alert('启动认证失败: ' + error);
  }
}

// 登出
function handleLogout() {
  authStore.logout();
  window.location.reload();
}

onMounted(() => {
  authStore.loadUserFromStorage();
});
</script>

<template>
  <div class="google-auth">
    <!-- 已登录状态 -->
    <div v-if="authStore.isAuthenticated" class="user-info">
      <el-dropdown>
        <div class="user-profile">
          <el-avatar :size="32" class="user-avatar">
            <img
              v-if="authStore.user?.picture"
              :src="authStore.user.picture"
              referrerpolicy="no-referrer"
              alt="avatar"
            />
          </el-avatar>
        </div>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item disabled>
              {{ authStore.user?.email }}
            </el-dropdown-item>
            <el-dropdown-item divided @click="handleLogout">
              <el-icon><Switch-button /></el-icon>
              退出登录
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
    
    <!-- 未登录状态 -->
    <div v-else class="login-section">
      <div v-if="!isGoogleAuthConfigured()" class="config-warning">
        <el-alert
          title="Google 认证未配置"
          type="warning"
          :closable="false"
          show-icon
        >
          <template #default>
            请在 .env 文件中设置 VITE_GOOGLE_CLIENT_ID
          </template>
        </el-alert>
      </div>
      <button
        v-else
        type="button"
        class="google-icon-btn"
        title="使用 Google 账号登录"
        @click="openGoogleAuth"
      >
        <svg viewBox="0 0 48 48" width="20" height="20">
          <path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"></path>
          <path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"></path>
          <path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z"></path>
          <path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"></path>
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.google-auth {
  display: flex;
  align-items: center;
}

.user-info {
  display: flex;
  align-items: center;
}

.user-profile {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 4px;
  transition: background-color 0.2s;
}

.user-profile:hover {
  background-color: #f5f7fa;
}

.user-avatar {
  border: 2px solid #e4e7ed;
}

.user-avatar :deep(img) {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.user-name {
  font-size: 14px;
  color: #606266;
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.login-section {
  display: flex;
  align-items: center;
}

.config-warning {
  width: 250px;
}

.google-icon-btn {
  width: 32px;
  height: 32px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: #fff;
  border: 1px solid #e4e7ed;
  border-radius: 50%;
  cursor: pointer;
  transition: box-shadow 0.2s, border-color 0.2s;
}

.google-icon-btn:hover {
  border-color: #c0c4cc;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}
</style>
