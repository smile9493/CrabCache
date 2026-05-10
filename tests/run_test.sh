#!/bin/bash

# CrabCache 缓存命中率测试快速启动脚本

set -e

echo "=========================================="
echo "CrabCache 缓存命中率测试工具"
echo "=========================================="
echo ""

# 检查 Python 环境
if ! command -v python3 &> /dev/null; then
    echo "❌ 错误: 未找到 Python3，请先安装 Python 3.8+"
    exit 1
fi

echo "✅ Python 版本: $(python3 --version)"

# 检查依赖
echo ""
echo "检查依赖包..."
pip3 install -q -r requirements.txt

if [ $? -eq 0 ]; then
    echo "✅ 依赖包已安装"
else
    echo "❌ 依赖包安装失败"
    exit 1
fi

# 检查网关服务
echo ""
echo "检查网关服务..."

if curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/v1/models | grep -q "200\|401"; then
    echo "✅ 网关服务运行中 (http://localhost:8080)"
else
    echo "⚠️  警告: 网关服务未运行或无法访问"
    echo "   请确保 CrabCache 网关已启动"
    read -p "是否继续测试？(y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# 检查 Metrics 服务
echo ""
echo "检查 Metrics 服务..."

if curl -s -o /dev/null -w "%{http_code}" http://localhost:9090/metrics | grep -q "200"; then
    echo "✅ Metrics 服务运行中 (http://localhost:9090)"
else
    echo "⚠️  警告: Metrics 服务未运行或无法访问"
fi

# 运行测试
echo ""
echo "=========================================="
echo "开始运行缓存命中率测试..."
echo "=========================================="
echo ""

python3 cache_hit_rate_test.py

# 检查测试结果
if [ $? -eq 0 ]; then
    echo ""
    echo "=========================================="
    echo "测试完成！"
    echo "=========================================="
    echo ""
    echo "测试报告:"
    echo "  - cache_test_report.md"
    echo "  - cache_test_visualization.png"
    echo ""
    
    # 显示报告摘要
    if [ -f cache_test_report.md ]; then
        echo "测试摘要:"
        grep -A 5 "## 测试摘要" cache_test_report.md | tail -n +2
    fi
else
    echo ""
    echo "❌ 测试失败，请检查错误日志"
    exit 1
fi
