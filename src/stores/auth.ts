import { ref, computed } from 'vue';
import { defineStore } from 'pinia';
import type { GoogleUserInfo } from '../config/googleAuth';

export const useAuthStore = defineStore('auth', () => {
  // State
  const user = ref<GoogleUserInfo | null>(null);
  const isAuthenticated = ref(false);
  const token = ref<string | null>(null);

  // Getters
  const getUser = computed(() => user.value);
  const getIsAuthenticated = computed(() => isAuthenticated.value);
  const getToken = computed(() => token.value);

  // Actions
  function setUser(userInfo: GoogleUserInfo) {
    user.value = userInfo;
    isAuthenticated.value = true;
    // 保存到 localStorage
    localStorage.setItem('adb_tool_user', JSON.stringify(userInfo));
  }

  function setToken(authToken: string) {
    token.value = authToken;
    localStorage.setItem('adb_tool_token', authToken);
  }

  function logout() {
    user.value = null;
    token.value = null;
    isAuthenticated.value = false;
    localStorage.removeItem('adb_tool_user');
    localStorage.removeItem('adb_tool_token');
  }

  function loadUserFromStorage() {
    const savedUser = localStorage.getItem('adb_tool_user');
    const savedToken = localStorage.getItem('adb_tool_token');
    
    if (savedUser) {
      try {
        user.value = JSON.parse(savedUser);
        isAuthenticated.value = true;
      } catch (e) {
        console.error('Failed to parse user from storage:', e);
      }
    }
    
    if (savedToken) {
      token.value = savedToken;
    }
  }

  // 解码 JWT token 获取用户信息
  function decodeCredential(credential: string): GoogleUserInfo | null {
    try {
      const base64Url = credential.split('.')[1];
      const base64 = base64Url.replace(/-/g, '+').replace(/_/g, '/');
      const jsonPayload = decodeURIComponent(
        atob(base64)
          .split('')
          .map((c) => '%' + ('00' + c.charCodeAt(0).toString(16)).slice(-2))
          .join('')
      );
      
      const payload = JSON.parse(jsonPayload);
      
      return {
        id: payload.sub,
        email: payload.email,
        name: payload.name,
        picture: payload.picture,
        givenName: payload.given_name,
        familyName: payload.family_name,
      };
    } catch (error) {
      console.error('Failed to decode credential:', error);
      return null;
    }
  }

  return {
    user,
    isAuthenticated,
    token,
    getUser,
    getIsAuthenticated,
    getToken,
    setUser,
    setToken,
    logout,
    loadUserFromStorage,
    decodeCredential,
  };
});
