import type { GlobalThemeOverrides } from 'naive-ui';

export const accentHex = '#34D399';
export const accentHoverHex = '#4ADE80';
export const accentPressedHex = '#16A34A';

export const accentLightHex = '#10B981';
export const accentLightHoverHex = '#059669';
export const accentLightPressedHex = '#047857';

const fontSans =
  '-apple-system, "SF Pro Text", "PingFang SC", "Inter", "Segoe UI", sans-serif';

export const darkOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: accentHex,
    primaryColorHover: accentHoverHex,
    primaryColorPressed: accentPressedHex,
    primaryColorSuppl: accentHex,
    borderRadius: '6px',
    borderRadiusSmall: '4px',
    bodyColor: '#0A0A0B',
    cardColor: '#131316',
    modalColor: '#1D1D22',
    popoverColor: '#1D1D22',
    textColor1: '#EDEDED',
    textColor2: '#EDEDED',
    textColor3: '#8B8B8B',
    placeholderColor: '#6A6A6F',
    dividerColor: '#1F1F23',
    borderColor: '#28282D',
    inputColor: '#18181C',
    inputColorDisabled: '#131316',
    tableColor: '#131316',
    tagColor: '#18181C',
    actionColor: '#18181C',
    fontFamily: fontSans
  },
  Card: {
    borderRadius: '8px',
    color: '#131316',
    colorEmbedded: '#131316',
    borderColor: '#1F1F23'
  },
  Button: {
    borderRadiusMedium: '6px',
    borderRadiusSmall: '4px',
    heightMedium: '32px',
    heightSmall: '28px'
  },
  Input: {
    borderRadius: '6px',
    color: '#18181C',
    colorFocus: '#18181C',
    border: '1px solid #28282D',
    borderHover: '1px solid #3a3a40',
    borderFocus: `1px solid ${accentHex}`,
    boxShadowFocus: `0 0 0 3px rgba(52,211,153,0.18)`
  },
  Tag: {
    borderRadius: '999px'
  },
  Modal: {
    color: '#1D1D22'
  },
  Slider: {
    fillColor: accentHex,
    fillColorHover: accentHoverHex,
    railColor: '#28282D',
    handleColor: accentHex
  }
};

export const lightOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: accentLightHex,
    primaryColorHover: accentLightHoverHex,
    primaryColorPressed: accentLightPressedHex,
    primaryColorSuppl: accentLightHex,
    borderRadius: '6px',
    borderRadiusSmall: '4px',
    bodyColor: '#FAFAF9',
    cardColor: '#FFFFFF',
    modalColor: '#FFFFFF',
    popoverColor: '#FFFFFF',
    textColor1: '#18181B',
    textColor2: '#18181B',
    textColor3: '#71717A',
    placeholderColor: '#A1A1AA',
    dividerColor: '#E4E4E7',
    borderColor: '#D4D4D8',
    inputColor: '#FFFFFF',
    inputColorDisabled: '#FAFAFA',
    tableColor: '#FFFFFF',
    tagColor: '#F4F4F5',
    actionColor: '#F4F4F5',
    fontFamily: fontSans
  },
  Card: {
    borderRadius: '8px',
    color: '#FFFFFF',
    colorEmbedded: '#FFFFFF',
    borderColor: '#E4E4E7'
  },
  Button: {
    borderRadiusMedium: '6px',
    borderRadiusSmall: '4px',
    heightMedium: '32px',
    heightSmall: '28px'
  },
  Input: {
    borderRadius: '6px',
    color: '#FFFFFF',
    colorFocus: '#FFFFFF',
    border: '1px solid #D4D4D8',
    borderHover: '1px solid #a1a1aa',
    borderFocus: `1px solid ${accentLightHex}`,
    boxShadowFocus: `0 0 0 3px rgba(16,185,129,0.18)`
  },
  Tag: {
    borderRadius: '999px'
  },
  Modal: {
    color: '#FFFFFF'
  },
  Slider: {
    fillColor: accentLightHex,
    fillColorHover: accentLightHoverHex,
    railColor: '#E4E4E7',
    handleColor: accentLightHex
  }
};
