// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

// ---------------------------------------------------------------------------
// Deploy the PoSq × Lighter demo contract suite to Sepolia.
//
// Order: BigMulMod -> MAIN PoSqHost (honest sequencer, funded bond)
//        -> SACRIFICIAL PoSqHost (malicious sequencer, funded so the live demo
//           can slash it) -> LighterBridgeDemo (pointed at the MAIN host).
//
// Env vars (set before invoking):
//   SEQ_ADDR         honest sequencer address for the main host       (required)
//   MALICIOUS_ADDR   sequencer address for the sacrificial host       (required)
//   BOND_WEI         wei to fund the main host bond   (default 0.05 ether)
//   SAC_BOND_WEI     wei to fund the sacrificial bond (default 0.02 ether)
//
// EXACT Sepolia invocation (does NOT run here — script only; broadcast is manual):
//
//   SEQ_ADDR=0x... MALICIOUS_ADDR=0x... BOND_WEI=50000000000000000 \
//   ~/.foundry/bin/forge script script/Deploy.s.sol:Deploy \
//     --rpc-url https://rpc.sepolia.org \
//     --private-key $(cat ../.sepolia-key) \
//     --broadcast
//
// (Drop --broadcast for a dry run. The deployer key funds both bonds, so it
//  must hold at least BOND_WEI + SAC_BOND_WEI + gas.)
// ---------------------------------------------------------------------------

import {Script} from "forge-std/Script.sol";
import {console2} from "forge-std/console2.sol";
import {PoSqHost, IBigMulMod} from "../src/PoSqHost.sol";
import {BigMulMod} from "../src/BigMulMod.sol";
import {LighterBridgeDemo} from "../src/LighterBridgeDemo.sol";

contract Deploy is Script {
    // RSA-2048 modulus N (crates/vdf Group::default_rsa2048()); the MAIN host
    // must carry this exact N so it can natively verify the real segment proofs
    // in vectors/segment.json (q_squarings * segment_ticks = 3600 * 256).
    bytes internal constant MODULUS_N =
        hex"c7970ceedcc3b0754490201a7aa613cd73911081c790f5f1a8726f463550bb5b"
        hex"7ff0db8e1ea1189ec72f93d1650011bd721aeeacc2acde32a04107f0648c2813"
        hex"a31f5b0b7765ff8b44b4b6ffc93384b646eb09c7cf5e8592d40ea33c80039f35"
        hex"b4f14a04b51f7bfd781be4d1673164ba8eb991c2c4d730bbbe35f592bdef524a"
        hex"f7e8daefd26c66fc02c479af89d64d373f442709439de66ceb955f3ea37d5159"
        hex"f6135809f85334b5cb1813addc80cd05609f10ac6a95ad65872c909525bdad32"
        hex"bc729592642920f24c61dc5b3c3b7923e56b16a4d9d373d8721f24a3fc0f1b31"
        hex"31f55615172866bccc30f95054c824e733a5eb6817f7bc16399d48c6361cc7e5";

    // Parity with the reference vector / Rust prover. EPOCH must equal the
    // epoch the live gateway signs anchors with (submitAnchor binds it into
    // the recovered message); a fresh gateway boots into epoch 0. Override
    // with EPOCH_ID for a gateway that has already rotated.
    uint64 internal constant Q_SQUARINGS = 3600;
    uint64 internal constant SEGMENT_TICKS = 256;
    uint64 internal constant F_FORCE = 256; // forced-inclusion headroom (ticks)
    uint64 internal constant CHALLENGE_WINDOW = 300; // seconds
    uint256 internal constant BOND_FLOOR = 0.01 ether;

    function run() external {
        require(MODULUS_N.length == 256, "N must be 256 bytes");

        address seq = vm.envAddress("SEQ_ADDR");
        address malicious = vm.envAddress("MALICIOUS_ADDR");
        uint256 bondWei = vm.envOr("BOND_WEI", uint256(0.05 ether));
        uint256 sacBondWei = vm.envOr("SAC_BOND_WEI", uint256(0.02 ether));
        uint64 epochId = uint64(vm.envOr("EPOCH_ID", uint256(0)));

        vm.startBroadcast();

        // 1. Wide-mulmod library for native Wesolowski verification.
        BigMulMod big = new BigMulMod();

        // 2. MAIN host — honest sequencer, funded bond.
        PoSqHost mainHost = new PoSqHost(
            seq,
            epochId,
            MODULUS_N,
            BOND_FLOOR,
            Q_SQUARINGS,
            SEGMENT_TICKS,
            F_FORCE,
            CHALLENGE_WINDOW,
            IBigMulMod(address(big))
        );
        if (bondWei > 0) {
            (bool okMain,) = address(mainHost).call{value: bondWei}("");
            require(okMain, "fund main bond");
        }

        // 3. SACRIFICIAL host — malicious sequencer; this bond gets slashed live.
        PoSqHost sacrificialHost = new PoSqHost(
            malicious,
            epochId,
            MODULUS_N,
            BOND_FLOOR,
            Q_SQUARINGS,
            SEGMENT_TICKS,
            F_FORCE,
            CHALLENGE_WINDOW,
            IBigMulMod(address(big))
        );
        if (sacBondWei > 0) {
            (bool okSac,) = address(sacrificialHost).call{value: sacBondWei}("");
            require(okSac, "fund sacrificial bond");
        }

        // 4. Bridge bound to the MAIN host.
        LighterBridgeDemo bridge = new LighterBridgeDemo(address(mainHost));

        vm.stopBroadcast();

        // --- Log everything clearly ---
        console2.log("== PoSq x Lighter demo deployment ==");
        console2.log("BigMulMod          :", address(big));
        console2.log("PoSqHost (MAIN)    :", address(mainHost));
        console2.log("  sequencer        :", seq);
        console2.log("  bond funded (wei):", bondWei);
        console2.log("PoSqHost (SACRIFI) :", address(sacrificialHost));
        console2.log("  sequencer (evil) :", malicious);
        console2.log("  bond funded (wei):", sacBondWei);
        console2.log("LighterBridgeDemo  :", address(bridge));

        // --- Persist to deployments/sepolia.json ---
        string memory obj = "deployment";
        vm.serializeAddress(obj, "bigMulMod", address(big));
        vm.serializeAddress(obj, "poSqHostMain", address(mainHost));
        vm.serializeAddress(obj, "poSqHostSacrificial", address(sacrificialHost));
        vm.serializeAddress(obj, "lighterBridgeDemo", address(bridge));
        vm.serializeAddress(obj, "sequencer", seq);
        vm.serializeAddress(obj, "maliciousSequencer", malicious);
        vm.serializeUint(obj, "mainBondWei", bondWei);
        vm.serializeUint(obj, "sacrificialBondWei", sacBondWei);
        vm.serializeUint(obj, "chainId", block.chainid);
        string memory json = vm.serializeUint(obj, "epoch", uint256(epochId));

        vm.writeJson(json, "./deployments/sepolia.json");
        console2.log("Wrote deployments/sepolia.json");
    }
}
