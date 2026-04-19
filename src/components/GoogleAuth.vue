<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useAuthStore } from '../stores/auth';
import { GOOGLE_AUTH_CONFIG, isGoogleAuthConfigured } from '../config/googleAuth';

const authStore = useAuthStore();

// 在浏览器中打开 Google 认证页面
async function openGoogleAuth() {
  // 使用 OAuth 2.0 Implicit Grant Flow
  const authUrl = `https://accounts.google.com/o/oauth2/v2/auth?client_id=${GOOGLE_AUTH_CONFIG.clientId}&redirect_uri=http://localhost:1420&response_type=token&scope=email%20profile%20openid&prompt=select_account`;
  
  console.log('Opening Google auth in browser:', authUrl);
  
  try {
    // 使用 Tauri 在系统浏览器中打开
    await invoke('open_url', { url: authUrl });
    
    // 提示用户
    alert('认证页面已在浏览器中打开。\n\n认证完成后，请复制浏览器地址栏中的 #credential=xxx 或 #access_token=xxx 部分，然后在应用控制台中执行:\n\nauthStore.setToken("你的 token");\nauthStore.setUser(authStore.decodeCredential("你的 token"));');
  } catch (error) {
    console.error('Failed to open browser:', error);
    alert('打开浏览器失败：' + error);
  }
}

// 登出
function handleLogout() {
  authStore.logout();
  window.location.reload();
}

onMounted(() => {
  // 从 localStorage 加载用户
  authStore.loadUserFromStorage();
  
  // 检查 URL hash 中是否有认证信息（重定向回来后的处理）
  if (window.location.hash) {
    const hashParams = new URLSearchParams(window.location.hash.substring(1));
    const credential = hashParams.get('credential');
    const accessToken = hashParams.get('access_token');
    
    if (credential) {
      console.log('Found credential in hash');
      handleLoginSuccess({ credential });
      // 清除 hash
      window.history.replaceState({}, document.title, window.location.pathname);
    } else if (accessToken) {
      console.log('Found access_token in hash');
      authStore.setToken(accessToken);
      // 清除 hash
      window.history.replaceState({}, document.title, window.location.pathname);
    }
  }
});
</script>

<template>
  <div class="google-auth">
    <!-- 已登录状态 -->
    <div v-if="authStore.isAuthenticated" class="user-info">
      <el-dropdown>
        <div class="user-profile">
          <el-avatar 
            :src="authStore.user?.picture" 
            :size="32"
            class="user-avatar"
          />
          <span class="user-name">{{ authStore.user?.name }}</span>
          <el-icon><Arrow-down /></el-icon>
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
      <div v-else class="login-button-container">
        <el-button 
          type="danger" 
          size="large"
          @click="openGoogleAuth"
        >
          <svg class="google-icon" viewBox="0 0 24 24">
            <path fill="currentColor" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/>
            <path fill="currentColor" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/>
            <path fill="currentColor" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/>
            <path fill="currentColor" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>
          </svg>
          使用 Google 账号登录
        </el-button>
        <p class="login-hint">点击后将在浏览器中打开认证页面</p>
      </div>
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

.login-button-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}

.google-icon {
  width: 18px;
  height: 18px;
  margin-right: 8px;
}

.login-hint {
  font-size: 12px;
  color: #909399;
  margin: 0;
}
</style>
