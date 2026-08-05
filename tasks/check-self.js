#!/usr/bin/env node

const fs = require('node:fs')
const path = require('node:path')
const crypto = require('node:crypto')
const { spawnSync } = require('node:child_process')
const { requireExternalCargoTarget } = require('./storage.js')

const rootDir = path.resolve(__dirname, '..')
const maxOutputBytes = 32 * 1024 * 1024
const inspectedCallable = {
	query: 'src/commands/output.rs#write_text_or_print',
	nodeId:
		'symbol/file~1default~1src~01commands~01output.rs/rs:src~1commands~1output.rs:fn#write_text_or_print',
	digest: 'd58ae8bd40f5c98774a5b5e223bd770ccf490209a264b2f06cfa6bbf39f3ee73',
	effectSource:
		'symbol/file~1default~1src~01commands~01output.rs/rs:src~1commands~1output.rs:fn#write_file'
}

const countBy = (items, field) => {
	const counts = new Map()
	for (const item of items ?? []) {
		const value = String(item?.[field] ?? 'unknown')
		counts.set(value, (counts.get(value) ?? 0) + 1)
	}
	return Object.fromEntries([...counts].sort(([left], [right]) => left.localeCompare(right)))
}

const collectSymbols = (roots) => {
	const pending = [...(roots ?? [])].reverse()
	const symbols = []
	while (pending.length > 0) {
		const symbol = pending.pop()
		symbols.push(symbol)
		for (const child of [...(symbol.children ?? [])].reverse()) pending.push(child)
	}
	return symbols
}

const digestJson = (value) =>
	crypto.createHash('sha256').update(JSON.stringify(value)).digest('hex')

const resolveInspectedCallable = (report) => {
	const target = report.targets?.find((candidate) => candidate.query === inspectedCallable.query)
	if (!target || target.nodes?.length !== 1) return undefined
	const nodeId = target.nodes[0]
	return { nodeId, callable: report.nodes?.[nodeId]?.callable }
}

const summarize = (id, report) => {
	switch (id) {
		case 'scan-code': {
			const symbols = collectSymbols(report.symbols)
			const callables = symbols.map((symbol) => symbol.callable).filter(Boolean)
			const signatures = callables.flatMap((callable) => callable.signatures ?? [])
			const parameters = signatures.flatMap((signature) => signature.parameters ?? [])
			const effects = callables.flatMap((callable) => callable.effects ?? [])
			const blockReasons = callables.flatMap((callable) => callable.block_reasons ?? [])
			return {
				files: report.stats?.files_scanned ?? 0,
				skipped: report.stats?.files_skipped ?? 0,
				symbols: report.stats?.symbols_found ?? 0,
				callables: callables.length,
				effects: countBy(effects, 'kind'),
				blockReasons: countBy(blockReasons, 'kind'),
				parameterConstructibility: countBy(parameters, 'constructibility'),
				receiverConstructibility: countBy(
					signatures.map((signature) => signature.receiver),
					'constructibility'
				)
			}
		}
		case 'check-code':
		case 'usage-code':
			return {
				findings: report.findings?.length ?? 0,
				gates: report.findings?.filter((finding) => finding.gates).length ?? 0,
				kinds: countBy(report.findings, 'kind')
			}
		case 'inspect-code': {
			const inspected = resolveInspectedCallable(report)
			return {
				targets: report.targets?.length ?? 0,
				nodes: Object.keys(report.nodes ?? {}).length,
				edges: report.edges?.length ?? 0,
				omitted: report.omitted ?? {},
				targetNode: inspected?.nodeId ?? null,
				callableDigest: inspected?.callable ? digestJson(inspected.callable) : null,
				effects: inspected?.callable?.effects ?? [],
				blockReasons: inspected?.callable?.block_reasons ?? []
			}
		}
		case 'lexicon-code':
			return report.stats ?? {}
		case 'tests-inventory':
			return {
				projects: report.projects?.length ?? 0,
				contexts:
					report.projects?.reduce(
						(total, project) => total + (project.contexts?.length ?? 0),
						0
					) ?? 0,
				scripts:
					report.projects?.reduce(
						(total, project) => total + (project.scripts?.length ?? 0),
						0
					) ?? 0,
				duplicateScripts: report.duplicate_scripts?.length ?? 0
			}
		case 'tests-witnesses':
			return report.summary ?? {}
		default:
			return {}
	}
}

const validateInspectedCallable = (report) => {
	const inspected = resolveInspectedCallable(report)
	if (!inspected) return ['did not resolve exactly one inspected callable']
	const failures = []
	if (inspected.nodeId !== inspectedCallable.nodeId) {
		failures.push(`resolved unexpected node ${inspected.nodeId}`)
	}
	if (!inspected.callable) return [...failures, 'omitted the structured callable contract']
	const digest = digestJson(inspected.callable)
	if (digest !== inspectedCallable.digest) {
		failures.push(`callable digest changed from ${inspectedCallable.digest} to ${digest}`)
	}
	const expectedEffect = inspected.callable.effects?.some(
		(effect) =>
			effect.kind === 'filesystem_write' &&
			effect.provenance?.kind === 'propagated' &&
			effect.provenance?.source_target === inspectedCallable.effectSource
	)
	if (!expectedEffect) failures.push('omitted the propagated filesystem-write effect')
	const expectedBlock = inspected.callable.block_reasons?.some(
		(reason) => reason.kind === 'requires_factory' && reason.subject === 'signature:0:parameter:1'
	)
	if (!expectedBlock) failures.push('omitted the exact Path factory block')
	return failures
}

const writePrivate = (destination, content) => {
	fs.writeFileSync(destination, content, { mode: 0o600 })
	fs.chmodSync(destination, 0o600)
}

const run = (command, args, options = {}) => {
	const result = spawnSync(command, args, {
		cwd: rootDir,
		encoding: 'utf8',
		env: { ...process.env, NO_COLOR: '1' },
		maxBuffer: maxOutputBytes
	})
	if (result.error) throw result.error
	if (result.signal) throw new Error(`${options.label ?? command} terminated by ${result.signal}`)
	return result
}

try {
	const targetDir = requireExternalCargoTarget(rootDir)
	const auditDir = path.join(targetDir, 'codeatlas-self-audit')
	fs.mkdirSync(auditDir, { recursive: true, mode: 0o700 })
	fs.chmodSync(auditDir, 0o700)

	const build = run('cargo', ['build', '--quiet', '--locked', '--bin', 'codeatlas'], {
		label: 'CodeAtlas self-audit build'
	})
	writePrivate(path.join(auditDir, 'build.stdout.log'), build.stdout)
	writePrivate(path.join(auditDir, 'build.stderr.log'), build.stderr)
	if (build.status !== 0) {
		throw new Error(`CodeAtlas self-audit build failed with exit ${build.status ?? 1}`)
	}

	const executable = path.join(
		targetDir,
		'debug',
		process.platform === 'win32' ? 'codeatlas.exe' : 'codeatlas'
	)
	const globalArgs = ['--root', '.', '--config', 'codeatlas.json']
	const commands = [
		{
			id: 'scan-code',
			args: ['scan', 'code', '--scope', 'source', '--all', '--format', 'json']
		},
		{ id: 'check-code', args: ['check', 'code', '--format', 'json'], check: true },
		{ id: 'usage-code', args: ['usage', 'code', '--format', 'json'] },
		{
			id: 'inspect-code',
			args: [
				'inspect',
				'code',
				inspectedCallable.query,
				'--depth',
				'1',
				'--direction',
				'outgoing',
				'--max-nodes',
				'32'
			],
			validate: validateInspectedCallable
		},
		{ id: 'lexicon-code', args: ['lexicon', 'code', '--format', 'json'] },
		{ id: 'tests-inventory', args: ['scan', 'tests', '--format', 'json'] },
		{
			id: 'tests-witnesses',
			args: ['check', 'tests', '--format', 'json'],
			check: true
		}
	]
	const failures = []
	const summaries = []

	for (const descriptor of commands) {
		const result = run(executable, [...globalArgs, ...descriptor.args], {
			label: descriptor.id
		})
		const stdoutBytes = Buffer.byteLength(result.stdout)
		const stderrBytes = Buffer.byteLength(result.stderr)
		writePrivate(path.join(auditDir, `${descriptor.id}.json`), result.stdout)
		writePrivate(path.join(auditDir, `${descriptor.id}.stderr.log`), result.stderr)

		let report
		try {
			report = JSON.parse(result.stdout)
		} catch (error) {
			failures.push(`${descriptor.id} did not emit valid JSON: ${error.message}`)
			continue
		}
		if (!Number.isInteger(report.schema_version)) {
			failures.push(`${descriptor.id} omitted an integer schema_version`)
		}
		for (const failure of descriptor.validate?.(report) ?? []) {
			failures.push(`${descriptor.id} ${failure}`)
		}
		if (result.status !== 0) {
			const reason = descriptor.check
				? 'reported gating findings'
				: `failed with exit ${result.status ?? 1}`
			failures.push(`${descriptor.id} ${reason}`)
		}

		const summary = summarize(descriptor.id, report)
		summaries.push({
			id: descriptor.id,
			exitCode: result.status ?? 1,
			reportBytes: stdoutBytes,
			diagnosticBytes: stderrBytes,
			schemaVersion: report.schema_version,
			summary
		})
		console.log(`${descriptor.id}: ${JSON.stringify(summary)}`)
	}

	writePrivate(
		path.join(auditDir, 'summary.json'),
		`${JSON.stringify({ schemaVersion: 1, reports: summaries }, null, 2)}\n`
	)
	console.log(`CodeAtlas self-audit artifacts: ${auditDir}`)
	if (failures.length > 0) throw new Error(failures.join('\n'))
} catch (error) {
	console.error(error instanceof Error ? error.message : String(error))
	process.exitCode = 1
}
