import { value } from '@fixture/b'
import { featureValue } from '@fixture/b/features/feature.ts'
import { absoluteValue } from '/packages/b/src/absolute.ts'

export const combined = value + featureValue + absoluteValue
