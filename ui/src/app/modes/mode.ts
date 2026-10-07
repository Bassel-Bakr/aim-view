/**
 * The mode this build runs in. Each build configuration in angular.json (browser, server, desktop)
 * replaces this file with its mode.<name>.ts, so a build carries only its own mode's code. Without
 * one (the tests, a plain build), the app runs in the browser. In: nothing. Out: `MODE`, which
 * app.config.ts reads for the mode's providers and HttpClient features.
 */
export { MODE } from './mode.browser';
