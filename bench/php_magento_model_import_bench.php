<?php
/**
 * Same CSV, same attributes, but this time the import itself goes through
 * Magento's real product model/resource-model save pipeline --
 * `Magento\Catalog\Model\Product::save()` -- instead of this project's own
 * PDO code. This is the "how would someone actually import via Magento
 * itself" number: one full model load/validate/persist per row, EAV
 * attributes written one at a time by the framework's own resource model,
 * change-log entries queued for the by-schedule indexers (this install's
 * indexers are all "Update by Schedule", so save() does not pay a
 * synchronous full reindex -- otherwise this would be far slower still).
 *
 * Deliberately NOT batched, NOT bulk-upserted -- that's the entire point:
 * it measures what Magento's own abstraction actually costs per product,
 * which is exactly what the other three importers in this benchmark were
 * built to avoid paying.
 *
 * Usage:
 *   php bench/php_magento_model_import_bench.php --file products.csv \
 *     [--magento-root /path/to/magento] [--limit N]
 *
 * --limit caps how many CSV rows are imported (per-row model save is slow
 * enough that the full 1000-row fixture may not be practical to run
 * repeatedly; start small and scale up deliberately).
 */

declare(strict_types=1);

function parseModelArgs(array $argv): array
{
    $opts = ['limit' => null, 'magento_root' => getenv('HOME') . '/mage-postgres/magento', 'attribute_set' => 4];
    for ($i = 1; $i < count($argv); $i++) {
        if ($argv[$i] === '--file' && isset($argv[$i + 1])) {
            $opts['file'] = $argv[++$i];
        } elseif ($argv[$i] === '--limit' && isset($argv[$i + 1])) {
            $opts['limit'] = (int) $argv[++$i];
        } elseif ($argv[$i] === '--magento-root' && isset($argv[$i + 1])) {
            $opts['magento_root'] = $argv[++$i];
        } elseif ($argv[$i] === '--attribute-set' && isset($argv[$i + 1])) {
            $opts['attribute_set'] = (int) $argv[++$i];
        }
    }
    if (!isset($opts['file'])) {
        fwrite(STDERR, "usage: php php_magento_model_import_bench.php --file <csv> [--limit N] [--magento-root path]\n");
        exit(1);
    }
    return $opts;
}

function main(): void
{
    $opts = parseModelArgs($_SERVER['argv']);
    $totalStart = microtime(true);

    $bootstrapStart = microtime(true);
    require $opts['magento_root'] . '/app/bootstrap.php';
    $bootstrap = \Magento\Framework\App\Bootstrap::create($opts['magento_root'], $_SERVER);
    $objectManager = $bootstrap->getObjectManager();

    /** @var \Magento\Framework\App\State $state */
    $state = $objectManager->get(\Magento\Framework\App\State::class);
    try {
        $state->setAreaCode('adminhtml');
    } catch (\Magento\Framework\Exception\LocalizedException $e) {
        // Area code already set -- fine, only the first caller in a
        // process needs to set it.
    }
    $bootstrapTime = microtime(true) - $bootstrapStart;

    /** @var \Magento\Catalog\Model\ProductFactory $productFactory */
    $productFactory = $objectManager->get(\Magento\Catalog\Model\ProductFactory::class);

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

    $csvRows = [];
    while (($row = fgetcsv($handle, 0, ',', '"', '\\')) !== false) {
        $csvRows[] = $row;
        if ($opts['limit'] !== null && count($csvRows) >= $opts['limit']) {
            break;
        }
    }
    fclose($handle);
    $totalRows = count($csvRows);

    $created = 0;
    $updated = 0;
    $perProductTimes = [];
    $importStart = microtime(true);

    foreach ($csvRows as $row) {
        $rowStart = microtime(true);
        $data = array_combine($header, array_pad($row, count($header), ''));
        $sku = trim((string) $data['sku']);
        if ($sku === '') {
            continue;
        }

        /** @var \Magento\Catalog\Model\Product $product */
        $product = $productFactory->create();
        $existingId = $product->getIdBySku($sku);
        if ($existingId) {
            $product->load($existingId);
            $updated++;
        } else {
            $created++;
        }

        $product->setSku($sku);
        $product->setAttributeSetId($opts['attribute_set']);
        $product->setTypeId('simple');
        $product->setStoreId(0);
        $product->setStatus(1);
        $product->setVisibility(4);

        foreach ($data as $code => $value) {
            if ($code === 'sku' || $value === '') {
                continue;
            }
            $product->setData($code, $value);
        }

        $product->save();
        $perProductTimes[] = microtime(true) - $rowStart;
    }

    $importTime = microtime(true) - $importStart;
    $totalTime = microtime(true) - $totalStart;
    $avgPerProduct = $totalRows > 0 ? $importTime / $totalRows : 0;

    printf("=== PHP (Magento Model) Import Performance ===\n");
    printf("Rows processed:   %d (created=%d, updated=%d)\n", $totalRows, $created, $updated);
    printf("Bootstrap time:   %.3fs\n", $bootstrapTime);
    printf("Import time:      %.3fs (avg %.1fms/product)\n", $importTime, $avgPerProduct * 1000);
    printf("Total time:       %.3fs\n", $totalTime);
    printf("===============================================\n");
}

main();
