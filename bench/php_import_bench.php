<?php
/**
 * Minimal PHP/PDO product importer -- a third data point for the
 * Go-vs-Rust bulk-import benchmark documented in the top-level README.
 *
 * Deliberately scoped to match the other two benchmarks exactly, not a
 * general-purpose importer and not real Magento's own import mechanism
 * (which carries indexer/observer/plugin overhead neither the Go nor the
 * Rust benchmark pays either): parse the same CSV shape, resolve SKUs,
 * batch-insert new `catalog_product_entity` rows, bucket attribute values
 * by backend_type, and batch-upsert the 5 EAV value tables -- one explicit
 * transaction per table, multi-row `INSERT ... ON DUPLICATE KEY UPDATE`,
 * same as Go's --raw-sql mode and Rust's importer. No framework, no ORM,
 * plain PDO -- the fairest "what does this cost in PHP itself" comparison.
 *
 * Usage:
 *   php bench/php_import_bench.php --file products.csv --batch-size 500
 *
 * Reads MYSQL_HOST/MYSQL_PORT/MYSQL_USER/MYSQL_PASS/MYSQL_DB from the
 * environment, same convention as GoGento and gogento-rust.
 */

declare(strict_types=1);

function parseArgs(array $argv): array
{
    $opts = ['batch-size' => 500, 'store' => 0, 'attribute-set' => 4];
    for ($i = 1; $i < count($argv); $i++) {
        if ($argv[$i] === '--file' && isset($argv[$i + 1])) {
            $opts['file'] = $argv[++$i];
        } elseif ($argv[$i] === '--batch-size' && isset($argv[$i + 1])) {
            $opts['batch-size'] = (int) $argv[++$i];
        } elseif ($argv[$i] === '--store' && isset($argv[$i + 1])) {
            $opts['store'] = (int) $argv[++$i];
        } elseif ($argv[$i] === '--attribute-set' && isset($argv[$i + 1])) {
            $opts['attribute-set'] = (int) $argv[++$i];
        }
    }
    if (!isset($opts['file'])) {
        fwrite(STDERR, "usage: php php_import_bench.php --file <csv> [--batch-size 500] [--store 0] [--attribute-set 4]\n");
        exit(1);
    }
    return $opts;
}

function connect(): PDO
{
    $host = getenv('MYSQL_HOST') ?: 'localhost';
    $port = getenv('MYSQL_PORT') ?: '3306';
    $user = getenv('MYSQL_USER') ?: 'magento';
    $pass = getenv('MYSQL_PASS') ?: 'magento';
    $db = getenv('MYSQL_DB') ?: 'magento';

    $pdo = new PDO("mysql:host={$host};port={$port};dbname={$db};charset=utf8mb4", $user, $pass, [
        PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,
        PDO::ATTR_EMULATE_PREPARES => true,
    ]);
    return $pdo;
}

/** @return array<string, array{id:int, type:string}> attribute_code -> {id, backend_type} */
function loadAttributes(PDO $pdo): array
{
    $stmt = $pdo->query('SELECT attribute_id, attribute_code, backend_type FROM eav_attribute WHERE entity_type_id = 4');
    $map = [];
    foreach ($stmt->fetchAll(PDO::FETCH_ASSOC) as $row) {
        if (in_array($row['backend_type'], ['varchar', 'int', 'decimal', 'text', 'datetime'], true)) {
            $map[$row['attribute_code']] = ['id' => (int) $row['attribute_id'], 'type' => $row['backend_type']];
        }
    }
    return $map;
}

/** @param string[] $skus @return array<string,int> sku -> entity_id */
function lookupExistingSkus(PDO $pdo, array $skus, int $batchSize): array
{
    $map = [];
    if (empty($skus)) {
        return $map;
    }
    foreach (array_chunk($skus, max(1, $batchSize)) as $chunk) {
        $placeholders = implode(',', array_fill(0, count($chunk), '?'));
        $stmt = $pdo->prepare("SELECT entity_id, sku FROM catalog_product_entity WHERE sku IN ({$placeholders})");
        $stmt->execute($chunk);
        foreach ($stmt->fetchAll(PDO::FETCH_ASSOC) as $row) {
            $map[$row['sku']] = (int) $row['entity_id'];
        }
    }
    return $map;
}

/**
 * Bulk-inserts new entities, relying on MySQL/InnoDB's consecutive-
 * auto-increment-lock guarantee for a multi-row INSERT -- same trick as
 * the Go and Rust importers.
 *
 * @param array<int,array{sku:string,type_id:string}> $entries
 * @return array<string,int> sku -> entity_id
 */
function insertNewProducts(PDO $pdo, array $entries, int $attributeSetId, int $batchSize): array
{
    $map = [];
    if (empty($entries)) {
        return $map;
    }
    foreach (array_chunk($entries, max(1, $batchSize)) as $chunk) {
        $values = [];
        $params = [];
        foreach ($chunk as $entry) {
            $values[] = '(?, ?, ?)';
            $params[] = $attributeSetId;
            $params[] = $entry['type_id'];
            $params[] = $entry['sku'];
        }
        $sql = 'INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES ' . implode(',', $values);
        $pdo->prepare($sql)->execute($params);
        $firstId = (int) $pdo->lastInsertId();
        foreach ($chunk as $i => $entry) {
            $map[$entry['sku']] = $firstId + $i;
        }
    }
    return $map;
}

/**
 * Batched upsert into one of the 5 EAV value tables: one explicit
 * transaction wrapping every chunk (matching the same fix this project's
 * Go and Rust importers both needed -- see the README's benchmark section
 * on why per-chunk auto-commit roughly doubled DB time on both sides).
 *
 * @param array<int,array{entity_id:int,attribute_id:int,store_id:int,value:mixed}> $rows
 */
function flushEavTable(PDO $pdo, string $table, array $rows, int $batchSize): void
{
    if (empty($rows)) {
        return;
    }
    $pdo->beginTransaction();
    foreach (array_chunk($rows, max(1, $batchSize)) as $chunk) {
        $values = [];
        $params = [];
        foreach ($chunk as $row) {
            $values[] = '(?, ?, ?, ?)';
            $params[] = $row['entity_id'];
            $params[] = $row['attribute_id'];
            $params[] = $row['store_id'];
            $params[] = $row['value'];
        }
        $sql = "INSERT INTO {$table} (entity_id, attribute_id, store_id, value) VALUES " . implode(',', $values)
            . ' ON DUPLICATE KEY UPDATE value = VALUES(value)';
        $pdo->prepare($sql)->execute($params);
    }
    $pdo->commit();
}

function main(): void
{
    $opts = parseArgs($_SERVER['argv']);
    $totalStart = microtime(true);

    $handle = fopen($opts['file'], 'r');
    if ($handle === false) {
        fwrite(STDERR, "failed to open {$opts['file']}\n");
        exit(1);
    }
    $header = fgetcsv($handle, 0, ",", "\"", "\\");
    if ($header === false) {
        fwrite(STDERR, "empty CSV\n");
        exit(1);
    }
    $skuCol = array_search('sku', $header, true);
    if ($skuCol === false) {
        fwrite(STDERR, "CSV must contain a 'sku' column\n");
        exit(1);
    }
    $typeCol = array_search('type_id', $header, true);

    $csvRows = [];
    while (($row = fgetcsv($handle, 0, ",", "\"", "\\")) !== false) {
        $csvRows[] = $row;
    }
    fclose($handle);
    $totalRows = count($csvRows);

    $dbTime = 0.0;
    $pdo = connect();

    $attrsStart = microtime(true);
    $attrs = loadAttributes($pdo);
    $dbTime += microtime(true) - $attrsStart;

    $processStart = microtime(true);
    $skuType = [];
    $skus = [];
    foreach ($csvRows as $row) {
        $sku = trim((string) ($row[$skuCol] ?? ''));
        if ($sku === '') {
            continue;
        }
        $skus[] = $sku;
        if (!isset($skuType[$sku])) {
            $skuType[$sku] = ($typeCol !== false && isset($row[$typeCol]) && $row[$typeCol] !== '') ? $row[$typeCol] : 'simple';
        }
    }
    $skus = array_values(array_unique($skus));
    $processTime = microtime(true) - $processStart;

    $lookupStart = microtime(true);
    $skuToId = lookupExistingSkus($pdo, $skus, $opts['batch-size']);
    $updatedCount = 0;
    foreach (array_keys($skuType) as $sku) {
        if (isset($skuToId[$sku])) {
            $updatedCount++;
        }
    }
    $newEntries = [];
    foreach ($skuType as $sku => $typeId) {
        if (!isset($skuToId[$sku])) {
            $newEntries[] = ['sku' => $sku, 'type_id' => $typeId];
        }
    }
    $createdCount = count($newEntries);
    if (!empty($newEntries)) {
        $inserted = insertNewProducts($pdo, $newEntries, $opts['attribute-set'], $opts['batch-size']);
        $skuToId += $inserted;
    }
    $dbTime += microtime(true) - $lookupStart;

    $bucketStart = microtime(true);
    $buckets = ['varchar' => [], 'int' => [], 'decimal' => [], 'text' => [], 'datetime' => []];
    foreach ($csvRows as $row) {
        $sku = trim((string) ($row[$skuCol] ?? ''));
        if ($sku === '' || !isset($skuToId[$sku])) {
            continue;
        }
        $entityId = $skuToId[$sku];
        foreach ($header as $colIndex => $code) {
            if (!isset($attrs[$code])) {
                continue;
            }
            $value = $row[$colIndex] ?? '';
            if ($value === '') {
                continue;
            }
            $buckets[$attrs[$code]['type']][] = [
                'entity_id' => $entityId,
                'attribute_id' => $attrs[$code]['id'],
                'store_id' => $opts['store'],
                'value' => $value,
            ];
        }
    }
    $processTime += microtime(true) - $bucketStart;

    $flushStart = microtime(true);
    flushEavTable($pdo, 'catalog_product_entity_varchar', $buckets['varchar'], $opts['batch-size']);
    flushEavTable($pdo, 'catalog_product_entity_int', $buckets['int'], $opts['batch-size']);
    flushEavTable($pdo, 'catalog_product_entity_decimal', $buckets['decimal'], $opts['batch-size']);
    flushEavTable($pdo, 'catalog_product_entity_text', $buckets['text'], $opts['batch-size']);
    flushEavTable($pdo, 'catalog_product_entity_datetime', $buckets['datetime'], $opts['batch-size']);
    $dbTime += microtime(true) - $flushStart;

    $totalTime = microtime(true) - $totalStart;
    $eavTotal = array_sum(array_map('count', $buckets));

    printf("=== PHP Import Performance ===\n");
    printf("Rows in CSV:    %d\n", $totalRows);
    printf("Products:       %d created, %d updated\n", $createdCount, $updatedCount);
    printf(
        "EAV rows:       %d (varchar=%d int=%d decimal=%d text=%d datetime=%d)\n",
        $eavTotal,
        count($buckets['varchar']),
        count($buckets['int']),
        count($buckets['decimal']),
        count($buckets['text']),
        count($buckets['datetime'])
    );
    printf("Total time:     %.3fs\n", $totalTime);
    printf("  - Processing: %.3fs\n", $processTime);
    printf("  - DB time:    %.3fs\n", $dbTime);
    $rate = $totalTime > 0 ? $createdCount / $totalTime : 0;
    printf("Rate:           %.0f products/sec\n", $rate);
    printf("===============================\n");
}

// Guards direct execution so php_magento_bootstrap_import_bench.php can
// `require` this file purely for its functions (connect(), parseArgs(),
// insertNewProducts(), flushEavTable(), ...) without also running main().
if (!defined('PHP_IMPORT_BENCH_LIBRARY_ONLY')) {
    main();
}
