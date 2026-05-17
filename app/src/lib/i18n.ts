import i18next from 'i18next';
import en from '../locales/en.json';

export async function initI18n(locale: string = 'en'): Promise<void> {
  await i18next.init({
    lng: locale,
    fallbackLng: 'en',
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
    returnNull: false,
  });
}

export function t(key: string, params?: Record<string, string | number>): string {
  return i18next.t(key, params) as string;
}
