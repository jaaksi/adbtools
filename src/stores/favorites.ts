import { ref, computed } from "vue";
import { defineStore } from "pinia";

const FAVS_KEY = "adbtools:favoritePackages";
const LAST_FAV_KEY = "adbtools:lastFavoritedPackage";

function loadList(): string[] {
  try {
    const s = localStorage.getItem(FAVS_KEY);
    return s ? JSON.parse(s) : [];
  } catch {
    return [];
  }
}

export const useFavoritesStore = defineStore("favorites", () => {
  const favorites = ref<string[]>(loadList());
  const lastFavorited = ref<string>(localStorage.getItem(LAST_FAV_KEY) ?? "");

  const hasFavorites = computed(() => favorites.value.length > 0);

  function persist() {
    localStorage.setItem(FAVS_KEY, JSON.stringify(favorites.value));
  }

  function isFavorite(pkg: string) {
    return favorites.value.includes(pkg);
  }

  function toggle(pkg: string): boolean {
    const idx = favorites.value.indexOf(pkg);
    if (idx >= 0) {
      favorites.value.splice(idx, 1);
      persist();
      return false;
    }
    favorites.value.push(pkg);
    lastFavorited.value = pkg;
    localStorage.setItem(LAST_FAV_KEY, pkg);
    persist();
    return true;
  }

  return { favorites, lastFavorited, hasFavorites, isFavorite, toggle };
});
