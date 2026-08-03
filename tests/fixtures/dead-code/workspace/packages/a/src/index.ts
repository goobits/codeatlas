import { value } from '@fixture/b'
import { featureValue } from '@fixture/b/features/feature.ts'
import { aliasShared } from '@fixture/aliased-shared'
import { message } from '@i18n/messages'
import { absoluteValue } from '/packages/b/src/absolute.ts'
import '/shared/browserRuntime.ts'
import { sharedRuntime } from '../../b/src/sharedRuntime.ts'
import '@fixture/b/generated'
import '@fixture/b/private'

export const combined = value + featureValue + aliasShared + absoluteValue + sharedRuntime() + message
