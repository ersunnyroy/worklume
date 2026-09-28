/** @type {import('jest').Config} */
module.exports = {
  preset: 'jest-expo',
  restoreMocks: true,
  testPathIgnorePatterns: ['/node_modules/', '/android/', '/ios/', '/dist/'],
};
