import { connectPluginV2 } from './pluginClientV2';
import { createPluginHost } from './pluginHost';

export const host = createPluginHost(connectPluginV2);
