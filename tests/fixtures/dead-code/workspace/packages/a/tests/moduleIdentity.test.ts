export async function hasSharedIdentity(): Promise<boolean> {
  const [packageModule, sourceModule] = await Promise.all([
    import('@fixture/b'),
    import('../../b/src/index.ts'),
  ])
  return packageModule === sourceModule
}
