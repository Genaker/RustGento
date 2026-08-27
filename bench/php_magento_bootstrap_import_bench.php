<?php
/**
 * Same benchmark as php_import_bench.php -- identical CSV parsing, SKU
 * resolution, and batched-upsert import logic, reused unchanged via
 * `require` -- except this variant first boots the real Magento 2
 * application (`\Magento\Framework\App\Bootstrap::create()`: DI container,
 * module/plugin graph, EAV/config caches) before running the import.
 *
 * The import itself still writes through this project's own plain PDO
 * connection, not Magento's resource models/ORM -- the point isn't to
 * benchmark Magento's ORM, it's to isolate what booting the framework
 * itself costs (time, memory) as a fixed tax paid before any import logic
 * runs at all, on top of the already-measured framework-free PHP numbers.
 *
 * Requires a real Magento 2 install with its schema already migrated
 * (`setup:install` already run) -- this repo doesn't ship one. Points at
 * `~/mage-postgres/magento` by default (override with --magento-root),
 * and that install's db connection must itself point at a real MySQL
 * instance (not the custom Postgres adapter some environments configure)
 * for this script's own separate PDO connection and Magento's connection
 * to agree on what "the database" is.
 *
 * Usage:
 *   php bench/php_magento_bootstrap_import_bench.php --file products.csv \
 *     [--magento-root /path/to/magento] [--batch-size 500]
 */

declare(strict_types=1);

define('PHP_IMPORT_BENCH_LIBRARY_ONLY', true);
require __DIR__ . '/php_import_bench.php';

function parseMagentoRoot(array $argv): string
{
    for ($i = 1; $i < count($argv); $i++) {
        if ($argv[$i] === '--magento-root' && isset($argv[$i + 1])) {
            return $argv[$i + 1];
        }
    }
    return getenv('HOME') . '/mage-postgres/magento';
}

function bootstrapMagento(string $magentoRoot): \Magento\Framework\App\Bootstrap
{
    require $magentoRoot . '/app/bootstrap.php';
    return \Magento\Framework\App\Bootstrap::create($magentoRoot, $_SERVER);
}

function main2(): void
{
    $opts = parseArgs($_SERVER['argv']);
    $magentoRoot = parseMagentoRoot($_SERVER['argv']);

    $totalStart = microtime(true);

    $bootstrapStart = microtime(true);
    $bootstrap = bootstrapMagento($magentoRoot);
    $objectManager = $bootstrap->getObjectManager();
    // Touch the object manager for something real -- resolving the EAV
    // config -- so the DI container/module graph is actually exercised,
    // not just instantiated and left untouched.
    $eavConfig = $objectManager->get(\Magento\Eav\Model\Config::class);
    $entityType = $eavConfig->getEntityType('catalog_product');
    $bootstrapTime = microtime(true) - $bootstrapStart;

    fwrite(STDERR, sprintf(
        "Magento bootstrapped: entity_type_id=%d (%.3fs)\n",
        (int) $entityType->getId(),
        $bootstrapTime
    ));

    // From here on, identical to php_import_bench.php's main() -- same
    // functions, same plain-PDO connection, deliberately not Magento's own
    // ResourceConnection/EAV resource models.
    $handle = fopen($opts['file'], 'r');
    if ($handle === false) {
        fwrite(STDERR, "failed to open {$opts['file']}\n");
        exit(1);
    }
    $header = fgetcsv($handle, 0, ',', '"', '\\');
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
    while (($row = fgetcsv($handle, 0, ',', '"', '\\')) !== false) {
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
    $importTime = $totalTime - $bootstrapTime;
    $eavTotal = array_sum(array_map('count', $buckets));

    printf("=== PHP (Magento-bootstrapped) Import Performance ===\n");
    printf("Rows in CSV:      %d\n", $totalRows);
    printf("Products:         %d created, %d updated\n", $createdCount, $updatedCount);
    printf(
        "EAV rows:         %d (varchar=%d int=%d decimal=%d text=%d datetime=%d)\n",
        $eavTotal,
        count($buckets['varchar']),
        count($buckets['int']),
        count($buckets['decimal']),
        count($buckets['text']),
        count($buckets['datetime'])
    );
    printf("Bootstrap time:   %.3fs\n", $bootstrapTime);
    printf("Import time:      %.3fs (processing=%.3fs, db=%.3fs)\n", $importTime, $processTime, $dbTime);
    printf("Total time:       %.3fs\n", $totalTime);
    printf("=====================================================\n");
}

main2();
