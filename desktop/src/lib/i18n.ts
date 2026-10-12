import strings from './en-US.json';
export const t = (key: keyof typeof strings): string => strings[key];
